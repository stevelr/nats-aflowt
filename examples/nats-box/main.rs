use nats_aflowt::{AsyncCall, BoxFuture};
use quicli::prelude::*;
use structopt::{clap::ArgGroup, StructOpt};

struct PrintCallback {
    msg: String,
}
impl PrintCallback {
    fn new(msg: &str) -> Self {
        Self {
            msg: msg.to_string(),
        }
    }
}
impl AsyncCall for PrintCallback {
    fn call(&self) -> BoxFuture<()> {
        let msg = self.msg.clone();
        Box::pin(async move {
            println!("{}", msg);
        })
    }
}

/// NATS utility that can perform basic publish, subscribe, request and reply
/// functions.
#[derive(Debug, StructOpt)]
#[structopt(group = ArgGroup::with_name("auth").required(false))]
struct Cli {
    /// NATS server
    #[structopt(long, short, default_value = "demo.nats.io")]
    server: nats_aflowt::ServerAddress,

    /// User Credentials File
    #[structopt(long = "creds", group = "auth")]
    creds: Option<String>,
    /// Server authorization token
    #[structopt(long = "auth-token", group = "auth")]
    auth_token: Option<String>,

    /// Command: pub, sub, request, reply
    #[structopt(subcommand)]
    cmd: Command,
}

#[derive(StructOpt, Debug, Clone)]
enum Command {
    /// The type of operation, can be one of pub, sub, qsub, req, reply.
    #[structopt(name = "pub", about = "Publishes a message to a given subject")]
    Pub { subject: String, msg: String },
    #[structopt(name = "sub", about = "Subscribes to a given subject")]
    Sub { subject: String },
    #[structopt(name = "request", about = "Sends a request and waits on reply")]
    Request { subject: String, msg: String },
    #[structopt(name = "reply", about = "Listens for requests and sends the reply")]
    Reply { subject: String, resp: String },
}

#[tokio::main]
async fn main() -> CliResult {
    let args = Cli::from_args();

    let opts = if let Some(creds_path) = args.creds {
        nats_aflowt::Options::with_credentials(creds_path)
    } else if let Some(token) = args.auth_token {
        nats_aflowt::Options::with_token(&token)
    } else {
        nats_aflowt::Options::new()
    };

    let nc = opts
        .with_name("nats-box rust example")
        .disconnect_callback(PrintCallback::new("Disconnected"))
        .reconnect_callback(PrintCallback::new("Reconnected"))
        .connect(args.server)
        .await?;

    match args.cmd {
        Command::Pub { subject, msg } => {
            nc.publish(&subject, &msg).await?;
            println!("Published to '{}': '{}'", subject, msg);
        }
        Command::Sub { subject } => {
            let sub = nc.subscribe(&subject).await?;
            println!("Listening on '{}'", subject);
            while let Some(msg) = sub.next().await {
                println!("Received a {:?}", msg);
            }
        }
        Command::Request { subject, msg } => {
            println!("Waiting on response for '{}'", subject);
            let resp = nc.request(&subject, &msg).await?;
            println!("Response is {:?}", resp);
        }
        Command::Reply { subject, resp } => {
            let sub = nc.queue_subscribe(&subject, "rust-box").await?;
            println!("Listening for requests on '{}'", subject);
            while let Some(msg) = sub.next().await {
                println!("Received a request {:?}", msg);
                msg.respond(&resp).await?;
            }
        }
    }

    Ok(())
}
