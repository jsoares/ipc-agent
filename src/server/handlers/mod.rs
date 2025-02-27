// Copyright 2022-2023 Protocol Labs
// SPDX-License-Identifier: MIT
//! The module contains the handlers implementation for the json rpc server.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::Value;

pub use config::ReloadConfigParams;
use fvm_shared::econ::TokenAmount;
use ipc_identity::PersistentKeyStore;
use manager::create::CreateSubnetHandler;
use manager::join::JoinSubnetHandler;
use manager::kill::KillSubnetHandler;
use manager::leave::LeaveSubnetHandler;
use manager::subnet::SubnetManagerPool;
pub use manager::*;

use crate::config::json_rpc_methods;
use crate::config::ReloadableConfig;
use crate::server::handlers::config::ReloadConfigHandler;
use crate::server::handlers::manager::fund::FundHandler;
use crate::server::handlers::manager::list_subnets::ListSubnetsHandler;
use crate::server::handlers::manager::propagate::PropagateHandler;
use crate::server::handlers::manager::release::ReleaseHandler;
use crate::server::handlers::manager::send_cross::SendCrossMsgHandler;
use crate::server::handlers::send_value::SendValueHandler;
use crate::server::handlers::validator::QueryValidatorSetHandler;
use crate::server::handlers::wallet::balances::WalletBalancesHandler;
use crate::server::handlers::wallet::new::WalletNewHandler;
use crate::server::list_checkpoints::ListBottomUpCheckpointsHandler;
use crate::server::net_addr::SetValidatorNetAddrHandler;
use crate::server::JsonRPCRequestHandler;
use ipc_identity::Wallet;

pub use self::config::{new_evm_keystore_from_config, new_evm_keystore_from_path};
pub use self::config::{new_fvm_wallet_from_config, new_keystore_from_path};
use self::rpc::RPCSubnetHandler;
use self::topdown_executed::LastTopDownExecHandler;
use self::wallet::export::WalletExportHandler;
use self::wallet::import::WalletImportHandler;
use self::wallet::remove::WalletRemoveHandler;

mod config;
mod manager;
mod validator;
pub mod wallet;

pub type Method = String;
/// We only support up to 9 decimal digits for transaction
const FIL_AMOUNT_NANO_DIGITS: u32 = 9;
/// The collection of all json rpc handlers
pub struct Handlers {
    handlers: HashMap<Method, Box<dyn HandlerWrapper>>,
}

/// A util trait to avoid Box<dyn> and associated type mess in Handlers struct
#[async_trait]
trait HandlerWrapper: Send + Sync {
    async fn handle(&self, params: Value) -> Result<Value>;
}

#[async_trait]
impl<H: JsonRPCRequestHandler + Send + Sync> HandlerWrapper for H {
    async fn handle(&self, params: Value) -> Result<Value> {
        let p = serde_json::from_value(params)?;
        let r = self.handle(p).await?;
        Ok(serde_json::to_value(r)?)
    }
}

impl Handlers {
    /// We test the handlers separately and individually instead of from the handlers.
    /// Convenient method for json rpc to test routing.
    #[cfg(test)]
    pub fn empty_handlers() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    pub fn new(
        config: Arc<ReloadableConfig>,
        fvm_wallet: Arc<RwLock<Wallet>>,
        evm_keystore: Arc<RwLock<PersistentKeyStore<ethers::types::Address>>>,
    ) -> Result<Self> {
        let mut handlers = HashMap::new();

        let h: Box<dyn HandlerWrapper> = Box::new(ReloadConfigHandler::new(config.clone()));
        handlers.insert(String::from(json_rpc_methods::RELOAD_CONFIG), h);

        // subnet manager methods
        let pool = Arc::new(SubnetManagerPool::new(
            config,
            fvm_wallet.clone(),
            evm_keystore.clone(),
        ));
        let h: Box<dyn HandlerWrapper> = Box::new(CreateSubnetHandler::new(pool.clone()));
        handlers.insert(String::from(json_rpc_methods::CREATE_SUBNET), h);

        let h: Box<dyn HandlerWrapper> = Box::new(LeaveSubnetHandler::new(pool.clone()));
        handlers.insert(String::from(json_rpc_methods::LEAVE_SUBNET), h);

        let h: Box<dyn HandlerWrapper> = Box::new(KillSubnetHandler::new(pool.clone()));
        handlers.insert(String::from(json_rpc_methods::KILL_SUBNET), h);

        let h: Box<dyn HandlerWrapper> = Box::new(JoinSubnetHandler::new(pool.clone()));
        handlers.insert(String::from(json_rpc_methods::JOIN_SUBNET), h);

        let h: Box<dyn HandlerWrapper> = Box::new(RPCSubnetHandler::new(pool.clone()));
        handlers.insert(String::from(json_rpc_methods::RPC_SUBNET), h);

        let h: Box<dyn HandlerWrapper> = Box::new(FundHandler::new(pool.clone()));
        handlers.insert(String::from(json_rpc_methods::FUND), h);

        let h: Box<dyn HandlerWrapper> = Box::new(ReleaseHandler::new(pool.clone()));
        handlers.insert(String::from(json_rpc_methods::RELEASE), h);

        let h: Box<dyn HandlerWrapper> = Box::new(PropagateHandler::new(pool.clone()));
        handlers.insert(String::from(json_rpc_methods::PROPAGATE), h);

        let h: Box<dyn HandlerWrapper> = Box::new(SendCrossMsgHandler::new(pool.clone()));
        handlers.insert(String::from(json_rpc_methods::SEND_CROSS_MSG), h);

        let h: Box<dyn HandlerWrapper> = Box::new(SendValueHandler::new(pool.clone()));
        handlers.insert(String::from(json_rpc_methods::SEND_VALUE), h);

        let h: Box<dyn HandlerWrapper> = Box::new(WalletNewHandler::new(
            fvm_wallet.clone(),
            evm_keystore.clone(),
        ));
        handlers.insert(String::from(json_rpc_methods::WALLET_NEW), h);

        let h: Box<dyn HandlerWrapper> = Box::new(WalletRemoveHandler::new(
            fvm_wallet.clone(),
            evm_keystore.clone(),
        ));
        handlers.insert(String::from(json_rpc_methods::WALLET_REMOVE), h);

        let h: Box<dyn HandlerWrapper> = Box::new(WalletImportHandler::new(
            fvm_wallet.clone(),
            evm_keystore.clone(),
        ));
        handlers.insert(String::from(json_rpc_methods::WALLET_IMPORT), h);

        let _h: Box<dyn HandlerWrapper> = Box::new(WalletExportHandler::new(
            fvm_wallet.clone(),
            evm_keystore.clone(),
        ));
        // FIXME: For security reasons currently not exposing the ability to export wallet
        // remotely through the RPC API, only directly through the CLI.
        // We can consider re-enabling once we have RPC authentication in the agent.
        // handlers.insert(String::from(json_rpc_methods::WALLET_EXPORT), h);

        let h: Box<dyn HandlerWrapper> = Box::new(WalletBalancesHandler::new(
            pool.clone(),
            fvm_wallet,
            evm_keystore,
        ));
        handlers.insert(String::from(json_rpc_methods::WALLET_BALANCES), h);

        let h: Box<dyn HandlerWrapper> = Box::new(SetValidatorNetAddrHandler::new(pool.clone()));
        handlers.insert(String::from(json_rpc_methods::SET_VALIDATOR_NET_ADDR), h);

        let h: Box<dyn HandlerWrapper> = Box::new(ListSubnetsHandler::new(pool.clone()));
        handlers.insert(String::from(json_rpc_methods::LIST_CHILD_SUBNETS), h);

        let h: Box<dyn HandlerWrapper> =
            Box::new(ListBottomUpCheckpointsHandler::new(pool.clone()));
        handlers.insert(String::from(json_rpc_methods::LIST_BOTTOMUP_CHECKPOINTS), h);

        let h: Box<dyn HandlerWrapper> = Box::new(LastTopDownExecHandler::new(pool.clone()));
        handlers.insert(String::from(json_rpc_methods::LAST_TOPDOWN_EXECUTED), h);

        // query validator
        let h: Box<dyn HandlerWrapper> = Box::new(QueryValidatorSetHandler::new(pool));
        handlers.insert(String::from(json_rpc_methods::QUERY_VALIDATOR_SET), h);

        Ok(Self { handlers })
    }

    pub async fn handle(&self, method: Method, params: Value) -> Result<Value> {
        if let Some(wrapper) = self.handlers.get(&method) {
            wrapper.handle(params).await
        } else {
            Err(anyhow!("method not supported"))
        }
    }
}

pub(crate) fn f64_to_token_amount(f: f64) -> anyhow::Result<TokenAmount> {
    // no rounding, just the integer part
    let nano = f64::trunc(f * (10u64.pow(FIL_AMOUNT_NANO_DIGITS) as f64));
    Ok(TokenAmount::from_nano(nano as u128))
}

#[cfg(test)]
mod tests {
    use crate::server::handlers::f64_to_token_amount;
    use fvm_shared::econ::TokenAmount;

    #[test]
    fn test_amount() {
        let amount = f64_to_token_amount(1000000.1f64).unwrap();
        assert_eq!(amount, TokenAmount::from_nano(1000000100000000u128));
    }
}
