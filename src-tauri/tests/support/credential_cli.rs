use std::{env, fs, io::Write, thread, time::Duration};

fn main() {
    let root = env::current_exe().unwrap().parent().unwrap().to_path_buf();
    let scenario = fs::read_to_string(root.join("scenario")).unwrap();
    match scenario.as_str() {
        "shim" => println!("please install gh"),
        "broken" => std::process::exit(42),
        "overflow" => {
            let mut output = std::io::stdout().lock();
            loop {
                if output.write_all(&[b'x'; 4096]).is_err() {
                    break;
                }
            }
        }
        "stalled" => thread::sleep(Duration::from_secs(30)),
        "success" | "signed-out" => {
            if env::args().nth(1).as_deref() == Some("--version") {
                println!("gh version 2.101.0\nhttps://github.com/cli/cli/releases/tag/v2.101.0");
            } else {
                assert_eq!(
                    env::args().skip(1).collect::<Vec<_>>(),
                    ["auth", "token", "--hostname", "github.com"]
                );
                if scenario == "signed-out" {
                    eprintln!("gho_secret_error_fixture");
                    std::process::exit(1);
                }
                println!("gho_fixture_not_a_real_token");
            }
        }
        _ => panic!("unknown fixture scenario"),
    }
}
