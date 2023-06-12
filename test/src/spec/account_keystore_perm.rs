use crate::setup::Setup;
use crate::spec::Spec;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

pub struct AccountKeystorePerm;

const CLI_PASSWORD: &str = "abc123456";

fn last_three_num(num: u32) -> u32 {
    num & 0o111
}

impl Spec for AccountKeystorePerm {
    fn run(&self, setup: &mut Setup) {
        let output = setup.cli_command(&["account", "new"], &[CLI_PASSWORD, CLI_PASSWORD]);
        assert!(output.contains("lock_arg: "));
        assert!(output.contains("lock_hash: "));
        println!("{}", output);
        println!("{}", setup.ckb_cli_dir);

        // print a number to octal
        println!(
            " 0o{:o}",
            fs::metadata(setup.ckb_cli_dir.clone())
                .unwrap()
                .permissions()
                .mode()
        );
        let keystore_path = PathBuf::from(setup.ckb_cli_dir.clone()).join("keystore");
        assert_eq!(
            last_three_num(fs::metadata(&keystore_path).unwrap().permissions().mode()),
            0o700
        );

        // iterator files under keystore_path
        fs::read_dir(keystore_path).unwrap().for_each(|file| {
            let file_path = file.unwrap().path();
            println!(
                " 0o{:o} {}",
                fs::metadata(&file_path).unwrap().permissions().mode(),
                file_path.display()
            );
            assert_eq!(
                last_three_num(fs::metadata(&file_path).unwrap().permissions().mode()),
                0o600
            );
        })
    }
}
