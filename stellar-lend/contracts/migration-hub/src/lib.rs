#![no_std]

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, log};

mod types;
mod adapter;

#[cfg(test)]
mod test;

use crate::types::{
    DataKey, MigrationAnalytics, MigrationConfig, MigrationError, MigrationPlan, MigrationRecord,
    MigrationStatus, ProtocolType,
};
use crate::adapter::{MigrationAdapter, StellarOtherLendAdapter};

#[contract]
pub struct MigrationHub;

#[contractimpl]
impl MigrationHub {
    pub fn initialize(
        env: Env,
        admin: Address,
        lending_contract: Address,
        bridge_contract: Address,
        rate_limit: u32,
        deadline: u64,
    ) -> Result<(), MigrationError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(MigrationError::AlreadyInitialized);
        }

        env.storage().instance().set(&DataKey::Admin, &admin);

        let config = MigrationConfig {
            lending_contract,
            bridge_contract,
            rate_limit_per_ledger: rate_limit,
            migration_deadline: deadline,
        };
        env.storage().instance().set(&DataKey::Config, &config);

        let analytics = MigrationAnalytics {
            total_migrated_value: 0,
            total_users: 0,
            successful_migrations: 0,
            failed_migrations: 0,
        };
        env.storage().instance().set(&DataKey::Analytics, &analytics);
        env.storage().instance().set(&DataKey::NextMigrationId, &0u64);

        Ok(())
    }

    pub fn approve_plan(
        env: Env,
        admin: Address,
        plan_id: BytesN<32>,
        old_contract: Address,
        new_contract: Address,
        state_root: BytesN<32>,
        total_steps: u32,
    ) -> Result<(), MigrationError> {
        Self::require_admin(&env, &admin)?;
        admin.require_auth();
        let plan = MigrationPlan {
            plan_id: plan_id.clone(),
            old_contract,
            new_contract,
            state_root,
            approved: true,
            total_steps,
            completed_steps: 0,
        };
        env.storage().persistent().set(&DataKey::Plan(plan_id), &plan);
        Ok(())
    }

    pub fn record_progress(
        env: Env,
        admin: Address,
        plan_id: BytesN<32>,
        completed_steps: u32,
    ) -> Result<(), MigrationError> {
        Self::require_admin(&env, &admin)?;
        admin.require_auth();
        let mut plan = Self::get_plan(env.clone(), plan_id.clone())?;
        if !plan.approved {
            return Err(MigrationError::MigrationNotApproved);
        }
        plan.completed_steps = completed_steps.min(plan.total_steps);
        env.storage().persistent().set(&DataKey::Plan(plan_id), &plan);
        Ok(())
    }

    pub fn get_plan(env: Env, plan_id: BytesN<32>) -> Result<MigrationPlan, MigrationError> {
        env.storage()
            .persistent()
            .get(&DataKey::Plan(plan_id))
            .ok_or(MigrationError::MigrationNotApproved)
    }

    pub fn rollback_migration(
        env: Env,
        admin: Address,
        migration_id: u64,
    ) -> Result<(), MigrationError> {
        Self::require_admin(&env, &admin)?;
        admin.require_auth();
        let mut record = Self::get_migration(env.clone(), migration_id)
            .ok_or(MigrationError::RollbackUnavailable)?;
        if record.status != MigrationStatus::Completed && record.status != MigrationStatus::Failed {
            return Err(MigrationError::RollbackUnavailable);
        }
        record.status = MigrationStatus::RolledBack;
        Self::save_migration(&env, migration_id, &record);
        Ok(())
    }

    pub fn migrate(
        env: Env,
        user: Address,
        protocol: ProtocolType,
        source_contract: Address,
        asset: Address,
        amount: i128,
    ) -> Result<u64, MigrationError> {
        user.require_auth();

        let config: MigrationConfig = env.storage().instance().get(&DataKey::Config).ok_or(MigrationError::NotInitialized)?;

        if env.ledger().timestamp() > config.migration_deadline {
            return Err(MigrationError::DeadlineExceeded);
        }

        let id = Self::get_next_id(&env);
        let mut record = MigrationRecord {
            user: user.clone(),
            protocol: protocol.clone(),
            asset: asset.clone(),
            amount,
            status: MigrationStatus::Pending,
            timestamp: env.ledger().timestamp(),
        };

        let result = match protocol {
            ProtocolType::StellarOther => {
                let adapter = StellarOtherLendAdapter { source_contract };
                adapter.pull_funds(&env, &user, &asset, amount)
            }
            ProtocolType::CrossChainBridge => Ok(()),
            ProtocolType::AaveMock => {
                let token = soroban_sdk::token::Client::new(&env, &asset);
                token.transfer(&user, &env.current_contract_address(), &amount);
                Ok(())
            }
        };

        if result.is_err() {
            record.status = MigrationStatus::Failed;
            Self::save_migration(&env, id, &record);
            Self::update_analytics(&env, false, 0);
            return Err(result.err().unwrap());
        }

        record.status = MigrationStatus::Completed;
        Self::save_migration(&env, id, &record);
        Self::update_analytics(&env, true, amount);

        log!(&env, "Migration successful for user {} amount {}", user, amount);

        Ok(id)
    }

    fn require_admin(env: &Env, admin: &Address) -> Result<(), MigrationError> {
        let current: Address = env.storage().instance().get(&DataKey::Admin).ok_or(MigrationError::NotInitialized)?;
        if current != *admin {
            return Err(MigrationError::Unauthorized);
        }
        Ok(())
    }

    fn get_next_id(env: &Env) -> u64 {
        let id: u64 = env.storage().instance().get(&DataKey::NextMigrationId).unwrap_or(0);
        env.storage().instance().set(&DataKey::NextMigrationId, &(id + 1));
        id
    }

    fn save_migration(env: &Env, id: u64, record: &MigrationRecord) {
        env.storage().persistent().set(&DataKey::Migration(id), record);
    }

    fn update_analytics(env: &Env, success: bool, amount: i128) {
        let mut stats: MigrationAnalytics = env.storage().instance().get(&DataKey::Analytics).unwrap();
        if success {
            stats.successful_migrations += 1;
            stats.total_migrated_value += amount;
            stats.total_users += 1;
        } else {
            stats.failed_migrations += 1;
        }
        env.storage().instance().set(&DataKey::Analytics, &stats);
    }

    pub fn get_analytics(env: Env) -> MigrationAnalytics {
        env.storage().instance().get(&DataKey::Analytics).unwrap()
    }

    pub fn get_migration(env: Env, id: u64) -> Option<MigrationRecord> {
        env.storage().persistent().get(&DataKey::Migration(id))
    }

    pub fn verify_migration(env: Env, migration_id: u64) -> Result<bool, MigrationError> {
        let record = Self::get_migration(env.clone(), migration_id).ok_or(MigrationError::MigrationFailed)?;

        if record.status != MigrationStatus::Completed {
            return Ok(false);
        }

        Ok(true)
    }
}
