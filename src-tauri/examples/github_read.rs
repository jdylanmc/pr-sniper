use pr_sniper_lib::github;

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() < 2 || args.len() > 3 || (args.len() == 3 && args[2] != "--metadata") {
        eprintln!("Usage: github_read owner/repository expected-decimal-account-id [--metadata]");
        std::process::exit(2);
    }
    let result = (|| {
        let client = github::client()?;
        let connection = client.connect(&args[0], Some(&args[1]))?;
        println!(
            "{}",
            serde_json::to_string(&connection)
                .map_err(|_| github::ConnectionError::InvalidResponse)?
        );
        if args.len() == 3 {
            let pulls = client.pull_requests(&connection.repository)?;
            println!(
                "{}",
                serde_json::json!({
                    "complete": true,
                    "pull_requests": pulls.len(),
                    "changed_files": pulls.iter().map(|pull| pull.files.len()).sum::<usize>(),
                    "numbers": pulls.iter().map(|pull| pull.number).collect::<Vec<_>>()
                })
            );
        }
        Ok::<_, github::ConnectionError>(())
    })();
    if let Err(error) = result {
        eprintln!("GitHub read failed: {error:?}");
        std::process::exit(1);
    }
}
