use crate::util::temp_dir;
use ckb_app_config::BlockAssemblerConfig;
use ckb_jsonrpc_types::{BlockTemplate, ProposalShortId};
use ckb_sdk::{Address, AddressPayload, CkbRpcClient, NetworkType};
use ckb_types::{
    core::BlockNumber,
    packed::{self, Block},
    prelude::*,
    H160, H256,
};
use std::fs;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

pub const DEFAULT_TX_PROPOSAL_WINDOW: (BlockNumber, BlockNumber) = (2, 10);
pub const MINER_PRIVATE_KEY: &str =
    "d00c06bfd800d27397002dca6fb0993d5ba6399b4238b2f29ee9deb97593d2bc";
pub const MINER_BLOCK_ASSEMBLER: &str = r#"
code_hash = "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8"
hash_type = "type"
args = "0xc8328aabcd9b9e8e64fbc566c4385c3bdeb219d7"
message = "0x"
"#;

pub struct Miner {
    rpc: Mutex<CkbRpcClient>,
    privkey_path: (TempDir, String),
}

impl Miner {
    pub fn init(uri: String) -> Self {
        let (tempdir, _) = temp_dir();
        let privkey_path = tempdir.path().join("pk");
        fs::write(&privkey_path, MINER_PRIVATE_KEY).unwrap();
        Self {
            rpc: Mutex::new(CkbRpcClient::new(uri.as_str())),
            privkey_path: (tempdir, privkey_path.to_string_lossy().to_string()),
        }
    }

    pub fn generate_block(&self) -> H256 {
        let template = self
            .rpc
            .lock()
            .unwrap()
            .get_block_template(None, None, None)
            .expect("RPC get_block_template");
        let work_id = template.work_id.value();
        let block = Into::<Block>::into(template);
        self.rpc
            .lock()
            .unwrap()
            .submit_block(work_id.to_string(), block.into())
            .expect("RPC submit_block")
    }

    pub fn generate_blocks(&self, count: u64) {
        log::info!("generating {} blocks...", count);
        (0..count).for_each(|_| {
            self.generate_block();
            thread::sleep(Duration::from_millis(10));
        })
    }

    pub fn mine_with_blocking<B>(&self, blocking: B) -> u64
    where
        B: Fn(&mut BlockTemplate) -> bool,
    {
        let mut count = 0;
        let mut template = self
            .rpc
            .lock()
            .unwrap()
            .get_block_template(None, None, None)
            .unwrap();
        while blocking(&mut template) {
            thread::sleep(Duration::from_millis(20));
            template = self
                .rpc
                .lock()
                .unwrap()
                .get_block_template(None, None, None)
                .unwrap();
            count += 1;

            if count > 900 {
                panic!("mine_with_blocking timeout");
            }
        }
        // uncles are not included by default,
        // because uncles' proposals can have an impact on the assertions of some tests
        let block = packed::Block::from(template)
            .as_advanced_builder()
            .set_uncles(vec![])
            .build();
        let number = block.number();
        self.rpc
            .lock()
            .unwrap()
            .submit_block("".to_owned(), block.data().into())
            .unwrap();
        number
    }
    pub fn mine_until_transaction_confirm_with_windows(
        &self,
        tx_hash: &packed::Byte32,
        closest: u64,
    ) {
        let target: ProposalShortId = packed::ProposalShortId::from_tx_hash(tx_hash).into();
        let last =
            self.mine_with_blocking(|template| !template.proposals.iter().any(|id| id == &target));
        self.mine_with_blocking(|template| template.number.value() != (last + closest - 1));
        self.mine_with_blocking(|template| {
            !template
                .transactions
                .iter()
                .any(|tx| tx.hash == tx_hash.unpack())
        });
    }
    pub fn mine_until_transaction_confirm(&self, tx_hash: &str) {
        log::info!("mine until tx: {}", tx_hash);
        let tx_hash: H256 = serde_json::from_str(&format!("\"{}\"", tx_hash)).unwrap();
        self.mine_until_transaction_confirm_with_windows(
            &tx_hash.pack(),
            DEFAULT_TX_PROPOSAL_WINDOW.0,
        )
    }

    pub fn privkey_path(&self) -> &str {
        &self.privkey_path.1
    }

    pub fn block_assembler() -> BlockAssemblerConfig {
        toml::from_str(MINER_BLOCK_ASSEMBLER).unwrap()
    }

    pub fn address() -> Address {
        let lock_arg = {
            let mut lock_arg = [0u8; 20];
            lock_arg.copy_from_slice(Self::block_assembler().args.as_bytes());
            H160(lock_arg)
        };
        let payload = AddressPayload::from_pubkey_hash(lock_arg);
        Address::new(NetworkType::Dev, payload, false)
    }
}
