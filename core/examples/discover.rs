//! Ask a DiskStation what it can do, without logging in.
//!
//! `SYNO.API.Info` needs no session, so this exercises the whole transport —
//! TLS, form encoding, the reply envelope — against a real box with no
//! credentials involved. It is the fastest way to tell "the address and port
//! are right" from "something else is wrong".
//!
//!     cargo run -p lookout-core --example discover -- HOST[:PORT] [--insecure]

use lookout_core::dsm::{self, Client, Host};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(target) = args.first() else {
        eprintln!("usage: discover HOST[:PORT] [--insecure]");
        std::process::exit(2);
    };

    let (address, port) = match target.rsplit_once(':') {
        Some((a, p)) => (a.to_owned(), p.parse().unwrap_or(5001)),
        None => (target.clone(), 5001),
    };

    let host = Host {
        address,
        port,
        https: true,
        verify_tls: !args.iter().any(|a| a == "--insecure"),
    };

    println!("querying {}", host.base_url());
    let client = Client::new(host);

    match dsm::discover(&client) {
        Ok(caps) => {
            println!("{} APIs\n", caps.len());
            for api in [
                "SYNO.API.Auth",
                "SYNO.Entry.Request",
                "SYNO.Core.System",
                "SYNO.Core.System.Utilization",
                "SYNO.Storage.CGI.Storage",
                "SYNO.Docker.Container",
                "SYNO.Core.SyslogClient.Log",
                // The name several published references use, which does not
                // exist. Printing it alongside the real one keeps the
                // distinction visible.
                "SYNO.Storage.CS.Storage",
            ] {
                let mark = if caps.has(api) { "yes" } else { " no" };
                let version = caps
                    .version_for(api, 7)
                    .map(|v| format!(" (call at v{v})"))
                    .unwrap_or_default();
                println!("  {mark}  {api}{version}");
            }
        }
        Err(e) => {
            eprintln!("failed: {e}");
            std::process::exit(1);
        }
    }
}
