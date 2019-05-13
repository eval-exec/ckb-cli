
use clap::{App, Arg, SubCommand, ArgMatches};
use std::env;
use std::iter::FromIterator;
use std::collections::HashMap;
use serde::de::DeserializeOwned;
use serde_json::to_string;
// use numext_fixed_hash::H256;
use jsonrpc_types::{
    CellOutPoint,
    OutPoint,
};

use json_color::Colorizer;

mod json_color;
mod rpc_client;

fn main() {
    let arg_hash = Arg::with_name("hash")
        .long("hash")
        .takes_value(true)
        .required(true);
    let arg_number = Arg::with_name("number")
        .long("number")
        .takes_value(true)
        .required(true)
        .help("Block number");

    let subcommand_rpc = SubCommand::with_name("rpc")
        .subcommands(vec![
            SubCommand::with_name("get_tip_header"),
            SubCommand::with_name("get_block")
                .arg(arg_hash.clone().help("Block hash")),
            SubCommand::with_name("get_block_hash")
                .arg(arg_number.clone()),
            SubCommand::with_name("get_transaction")
                .arg(arg_hash.clone().help("Tx hash")),
            SubCommand::with_name("get_cells_by_lock_hash")
                .arg(arg_hash.clone().help("Lock hash"))
                .arg(Arg::with_name("from")
                     .long("from")
                     .takes_value(true)
                     .required(true)
                     .help("From block number"))
                .arg(Arg::with_name("to")
                     .long("to")
                     .takes_value(true)
                     .required(true)
                     .help("To block number")),
            SubCommand::with_name("get_live_cell")
                .arg(arg_hash.clone().required(false).help("Block hash"))
                .arg(Arg::with_name("tx-hash")
                     .long("tx-hash")
                     .takes_value(true)
                     .required(true)
                     .help("Tx hash"))
                .arg(Arg::with_name("index")
                     .long("index")
                     .takes_value(true)
                     .required(true)
                     .help("Output index")),
            SubCommand::with_name("get_current_epoch"),
            SubCommand::with_name("get_epoch_by_number")
                .arg(arg_number.clone().help("Epoch number")),
            SubCommand::with_name("local_node_info"),
            SubCommand::with_name("tx_pool_info"),
            SubCommand::with_name("get_peers"),
        ]);

    let matches = App::new("ckb command line interface")
        .subcommand(subcommand_rpc)
        .arg(
            Arg::with_name("server")
                .long("server")
                .takes_value(true)
                .help("RPC server")
        )
        .get_matches();

    let mut env_map: HashMap<String, String> = HashMap::from_iter(env::vars());
    let server_uri = matches.value_of("server")
        .map(|value| value.to_string())
        .unwrap_or_else(|| env_map.remove("API_URL").unwrap());

    let mut client = rpc_client::RpcClient::from_uri(&server_uri);
    let mut content = "".to_string();
    match matches.subcommand() {
        ("rpc", Some(sub_matches)) => {
            match sub_matches.subcommand() {
                ("get_tip_header", _) => {
                    content = to_string(&client.get_tip_header().call().unwrap()).unwrap();
                }
                ("get_block", Some(m)) => {
                    let hash = from_matches(m, "hash");
                    content = to_string(&client.get_block(hash).call().unwrap()).unwrap();
                }
                ("get_block_hash", Some(m)) => {
                    let number = from_matches(m, "number");
                    content = to_string(
                        &client.get_block_hash(number).call().unwrap()
                    ).unwrap();
                }
                ("get_transaction", Some(m)) => {
                    let hash = from_matches(m, "hash");
                    content = to_string(&client.get_transaction(hash).call().unwrap()).unwrap();
                }
                ("get_cells_by_lock_hash", Some(m)) => {
                    let lock_hash = from_matches(m, "hash");
                    let from_number = from_matches(m, "from");
                    let to_number = from_matches(m, "to");
                    content = to_string(
                        &client.get_cells_by_lock_hash(lock_hash, from_number, to_number)
                            .call()
                            .unwrap()
                    ).unwrap();
                }
                ("get_live_cell", Some(m)) => {
                    let block_hash = from_matches_opt(m, "hash");
                    let tx_hash = from_matches(m, "tx-hash");
                    let index = from_matches(m, "index");
                    let out_point = OutPoint {
                        cell: Some(CellOutPoint {tx_hash, index}),
                        block_hash,
                    };
                    content = to_string(&client.get_live_cell(out_point).call().unwrap())
                        .unwrap();
                }
                ("get_current_epoch", _) => {
                    content = to_string(&client.get_current_epoch().call().unwrap()).unwrap();
                }
                ("local_node_info", _) => {
                    content = to_string(&client.local_node_info().call().unwrap()).unwrap();
                }
                ("tx_pool_info", _) => {
                    content = to_string(&client.tx_pool_info().call().unwrap()).unwrap();
                }
                ("get_peers", _) => {
                    content = to_string(&client.get_peers().call().unwrap()).unwrap();
                }
                _ => {
                    println!("Invalid command");
                }
            }
        }
        _ => {
            println!("Invalid command");
        }
    }

    if !content.is_empty() {
        let output = Colorizer::arbitrary()
            .colorize_json_str(content.as_str())
            .unwrap();
        println!("{}", output);
    }
}

fn from_string<T: DeserializeOwned>(source: String) -> T {
    let value = serde_json::Value::String(source);
    serde_json::from_value(value).unwrap()
}

fn from_matches<T>(matches: &ArgMatches, name: &str) -> T
where
    T: DeserializeOwned,
{
    from_string(matches.value_of(name).unwrap().to_string())
}

fn from_matches_opt<T>(matches: &ArgMatches, name: &str) -> Option<T>
where
    T: DeserializeOwned,
{
    matches.value_of(name).map(|hash_str| {
        from_string(hash_str.to_string())
    }).unwrap_or(None)
}
