// Copyright 2022-2023 Protocol Labs
// SPDX-License-Identifier: MIT
//! This mod contains the different command line implementations.

mod checkpoint;
mod config;
mod crossmsg;
mod daemon;
mod subnet;
mod util;
pub mod wallet;

use crate::cli::commands::checkpoint::CheckpointCommandsArgs;
use crate::cli::commands::crossmsg::CrossMsgsCommandsArgs;
use crate::cli::commands::daemon::{LaunchDaemon, LaunchDaemonArgs};
use crate::cli::commands::util::UtilCommandsArgs;
use crate::cli::{CommandLineHandler, GlobalArguments};
use crate::server::{new_evm_keystore_from_path, new_keystore_from_path};
use anyhow::{Context, Result};

use clap::{Command, CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Generator, Shell};
use ipc_identity::{KeyStore, PersistentKeyStore};

use std::fmt::Debug;
use std::io;
use subnet::SubnetCommandsArgs;
use url::Url;

use crate::cli::commands::config::ConfigCommandsArgs;
use crate::cli::commands::wallet::WalletCommandsArgs;

pub use subnet::*;

use super::default_repo_path;

/// The collection of all subcommands to be called, see clap's documentation for usage. Internal
/// to the current mode. Register a new command accordingly.
#[derive(Debug, Subcommand)]
enum Commands {
    /// Launch the ipc agent daemon.
    ///
    /// Note that, technically speaking, this just launches the ipc agent node and runs in the foreground
    /// and not in the background as what daemon processes are. Still, this struct contains `Daemon`
    /// due to the convention from `lotus` and the expected behavior from the filecoin user group.
    Daemon(LaunchDaemonArgs),
    Config(ConfigCommandsArgs),
    Subnet(SubnetCommandsArgs),
    Wallet(WalletCommandsArgs),
    CrossMsg(CrossMsgsCommandsArgs),
    Checkpoint(CheckpointCommandsArgs),
    Util(UtilCommandsArgs),
}

#[derive(Debug, Parser)]
#[command(
    name = "ipc-agent",
    about = "The IPC agent command line tool",
    version = "v0.0.1"
)]
#[command(propagate_version = true)]
struct IPCAgentCliCommands {
    // If provided, outputs the completion file for given shell
    #[arg(long = "cli-autocomplete-gen", value_enum)]
    generator: Option<Shell>,
    #[clap(flatten)]
    global_params: GlobalArguments,
    #[command(subcommand)]
    command: Option<Commands>,
}

/// The `cli` method exposed to handle all the cli commands, ideally from main.
///
/// # Examples
/// Sample usage:
/// ```ignore
/// # to start the daemon with
/// ipc-client daemon ./config/template.toml
/// ```
///
/// To register a new command, add the command to
/// ```ignore
/// pub async fn cli() {
///
///     // ... other code
///
///     let r = match &args.command {
///         // ... other existing commands
///         Commands::NewCommand => NewCommand::handle(n).await,
///     };
///
///     // ... other code
/// ```
/// Also add this type to Command enum.
/// ```ignore
/// enum Commands {
///     NewCommand(NewCommandArgs),
/// }
/// ```
pub async fn cli() -> anyhow::Result<()> {
    // parse the arguments
    let args = IPCAgentCliCommands::parse();

    if let Some(generator) = args.generator {
        let mut cmd = IPCAgentCliCommands::command();
        print_completions(generator, &mut cmd);
        Ok(())
    } else {
        let global = &args.global_params;
        if let Some(c) = &args.command {
            let r = match &c {
                Commands::Daemon(args) => LaunchDaemon::handle(global, args).await,
                Commands::Config(args) => args.handle(global).await,
                Commands::Subnet(args) => args.handle(global).await,
                Commands::CrossMsg(args) => args.handle(global).await,
                Commands::Wallet(args) => args.handle(global).await,
                Commands::Checkpoint(args) => args.handle(global).await,
                Commands::Util(args) => args.handle(global).await,
            };

            r.with_context(|| format!("error processing command {:?}", args.command))
        } else {
            Ok(())
        }
    }
}

fn print_completions<G: Generator>(gen: G, cmd: &mut Command) {
    generate(gen, cmd, cmd.get_name().to_string(), &mut io::stdout());
}

pub(crate) fn get_ipc_agent_url(
    ipc_agent_url: &Option<String>,
    global: &GlobalArguments,
) -> Result<Url> {
    let url = match ipc_agent_url {
        Some(url) => url.parse()?,
        None => {
            let config = global.config()?;
            let addr = config.server.json_rpc_address.to_string();
            // We are resolving back to our own ipc-agent node.
            // Since it's our own node, we will use http since we
            // should be in the same network.
            format!("http://{addr:}/json_rpc").parse()?
        }
    };
    Ok(url)
}

pub(crate) fn get_fvm_store(path: Option<String>) -> Result<KeyStore> {
    let path = match path {
        Some(p) => p,
        None => default_repo_path(),
    };
    new_keystore_from_path(&path)
}

pub(crate) fn get_evm_keystore(
    path: &Option<String>,
) -> Result<PersistentKeyStore<ethers::types::Address>> {
    match path {
        Some(p) => new_evm_keystore_from_path(p),
        None => new_evm_keystore_from_path(&default_repo_path()),
    }
}
