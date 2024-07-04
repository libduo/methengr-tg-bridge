//! Example to print the ID and title of all the dialogs.
//!
//! The `TG_ID` and `TG_HASH` environment variables must be set (learn how to do it for
//! [Windows](https://ss64.com/nt/set.html) or [Linux](https://ss64.com/bash/export.html))
//! to Telegram's API ID and API hash respectively.
//!
//! Then, run it as:
//!
//! ```sh
//! cargo run --example dialogs
//! ```

use futures_util::future::{select, Either};
use grammers_client::{Client, Config, SignInError, Update};
use grammers_session::Session;
use simple_logger::SimpleLogger;
use std::env;
use std::io::{self, BufRead as _, Write as _};
use std::pin::pin;
use tokio::runtime;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const SESSION_FILE: &str = "dialogs.session";

fn prompt(message: &str) -> Result<String> {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    stdout.write_all(message.as_bytes())?;
    stdout.flush()?;

    let stdin = io::stdin();
    let mut stdin = stdin.lock();

    let mut line = String::new();
    stdin.read_line(&mut line)?;
    Ok(line)
}

async fn handle_update(client: Client, update: Update) -> Result<()> {
    match update {
        Update::NewMessage(message) if !message.outgoing() => {
            let chat = message.chat();
            println!("Responding to {}", chat.name());
            client.send_message(&chat, message.text()).await?;
        }
        _ => {}
    }

    Ok(())
}

async fn async_main() -> Result<()> {
    let _ = dotenvy::dotenv();

    SimpleLogger::new()
        .with_level(log::LevelFilter::Debug)
        .init()
        .unwrap();

    let api_id = env::var("TG_ID")
        .expect("TG_ID unset")
        .parse()
        .expect("TG_ID invalid");
    let api_hash = env::var("TG_HASH").expect("TG_HASH unset");

    println!("Connecting to Telegram...");
    let client = Client::connect(Config {
        session: Session::load_file_or_create(SESSION_FILE)?,
        api_id,
        api_hash: api_hash.clone(),
        params: Default::default(),
    })
    .await?;
    println!("Connected!");

    // If we can't save the session, sign out once we're done.
    let mut sign_out = false;

    if !client.is_authorized().await? {
        println!("Signing in...");
        let phone = prompt("Enter your phone number (international format): ")?;
        let token = client.request_login_code(&phone).await?;
        let code = prompt("Enter the code you received: ")?;
        let signed_in = client.sign_in(&token, &code).await;
        match signed_in {
            Err(SignInError::PasswordRequired(password_token)) => {
                // Note: this `prompt` method will echo the password in the console.
                //       Real code might want to use a better way to handle this.
                let hint = password_token.hint().unwrap_or("None");
                let prompt_message = format!("Enter the password (hint {}): ", &hint);
                let password = prompt(prompt_message.as_str())?;

                client
                    .check_password(password_token, password.trim())
                    .await?;
            }
            Ok(_) => (),
            Err(e) => panic!("{}", e),
        };
        println!("Signed in!");
        match client.session().save_to_file(SESSION_FILE) {
            Ok(_) => {}
            Err(e) => {
                println!("NOTE: failed to save the session, will sign out when done: {e}");
                sign_out = true;
            }
        }
    }

    let mut dialogs = client.iter_dialogs();

    println!("Showing up to {} dialogs:", dialogs.total().await?);
    while let Some(dialog) = dialogs.next().await? {
        let chat = dialog.chat();
        println!("- {: >10} {}", chat.id(), chat.name());
    }

    loop {
        let update = {
            let exit = pin!(async { tokio::signal::ctrl_c().await });
            let upd = pin!(async { client.next_update().await });

            match select(exit, upd).await {
                Either::Left(_) => None,
                Either::Right((u, _)) => Some(u),
            }
        };

        let update = match update {
            None | Some(Ok(None)) => break,
            Some(u) => u?.unwrap(),
        };

        let handle = client.clone();
        tokio::spawn(async move {
            match handle_update(handle, update).await {
                Ok(_) => {}
                Err(e) => eprintln!("Error handling updates!: {e}"),
            }
        });
    }

    println!("sign out state: {}", sign_out);
    // if sign_out {
    //     // TODO revisit examples and get rid of "handle references" (also, this panics)
    //     drop(client.sign_out_disconnect().await);
    // }

    Ok(())
}

fn main() -> Result<()> {
    runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async_main())
}
