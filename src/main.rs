use std::path::{Path, PathBuf};

use acidjson::AcidJson;
use anyhow::Context;
use argh::FromArgs;
use once_cell::sync::Lazy;
use rand::{seq::SliceRandom, thread_rng};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use telegram_bot::{Response, TelegramBot};

/// raffle bot
#[derive(FromArgs, PartialEq, Debug)]
struct Args {
    /// configuration YAML file path
    #[argh(option, short = 'c', long = "config")]
    config: PathBuf,
}

/// The struct containing the bot configuration
#[derive(Serialize, Deserialize, Clone)]
struct Config {
    store_path: String,
    telegram_token: String,
    admin_uname: String,
    bot_uname: String,
}

static ARGS: Lazy<Args> = Lazy::new(argh::from_env);

static CONFIG: Lazy<Config> = Lazy::new(|| {
    let s = &std::fs::read(&ARGS.config).expect("cannot read config file");
    serde_yaml::from_slice(s).expect("cannot parse config file")
});

#[derive(Serialize, Deserialize, Clone)]
struct Store {
    giftcards: Vec<String>,
    participants: Vec<i64>, // list of all chat ids
}

async fn send_giftcards() -> anyhow::Result<()> {
    // shuffle participants list
    let mut store = STORE.read().clone();
    store.participants.shuffle(&mut thread_rng());
    for chat_id in store.participants {
        if let Some(gc) = store.giftcards.pop() {
            TELEGRAM
                .send_msg(Response {
                    text: "Congratulations! You won a giftcard! The code is:".into(),
                    chat_id,
                    reply_to_message_id: None,
                })
                .await?;
            TELEGRAM
                .send_msg(Response {
                    text: gc,
                    chat_id,
                    reply_to_message_id: None,
                })
                .await?;
        }
    }
    STORE.write().participants.clear();
    STORE.write().giftcards.clear();
    Ok(())
}

static STORE: Lazy<AcidJson<Store>> = Lazy::new(|| {
    AcidJson::open_or_else(Path::new(&CONFIG.store_path), || Store {
        giftcards: vec![],
        participants: vec![],
    })
    .unwrap()
});

static TELEGRAM: Lazy<TelegramBot> =
    Lazy::new(|| TelegramBot::new(&CONFIG.telegram_token, telegram_msg_handler));

#[derive(Deserialize, Serialize)]
struct StartRaffle {
    giftcards: Vec<String>,
}

async fn telegram_msg_handler(update: Value) -> anyhow::Result<Vec<Response>> {
    let admin_uname = &CONFIG.admin_uname;
    let msg = update["message"]["text"]
        .as_str()
        .context("cannot parse out text")?;
    log::info!("msg = {msg}");
    if update["message"]["chat"]["type"].as_str() == Some("private") {
        let mut username = "";
        if let Some(uname) = update["message"]["from"]["username"].as_str() {
            username = uname;
        };

        if username == admin_uname {
            // start raffle
            let maybe_start_raffle: Result<StartRaffle, _> = serde_json::from_str(msg);
            if let Ok(mut start_raffle) = maybe_start_raffle {
                STORE.write().giftcards.append(&mut start_raffle.giftcards);
                return Ok(to_response("Yay! The raffle has begun!", update)?);
            }
            // end raffle
            if msg == "#EndRaffle" {
                send_giftcards().await?;
                return Ok(to_response(
                    "Horray! We gave out all the gift cards!",
                    update,
                )?);
            }
            // display participants count
            if msg == "#ParticipantsCount" {
                let count = STORE.read().participants.len();
                return Ok(to_response(&count.to_string(), update)?);
            }
            // display giftcards count
            if msg == "#GiftcardsCount" {
                let count = STORE.read().giftcards.len();
                return Ok(to_response(&count.to_string(), update)?);
            }
        } else {
            if STORE.read().giftcards.is_empty() {
                // no ongoing raffle
                return Ok(
                    to_response("Sorry! There's no ongoing raffle at the moment. Watch out for future raffles in our user group!", update)?
                );
            } else {
                // exists ongoing raffle
                let chat_id = update["message"]["chat"]["id"]
                    .as_i64()
                    .context("could not get chat id")?;
                STORE.write().participants.push(chat_id);
                return Ok(to_response(
                    "Yay! You've been entered into the raffle!",
                    update,
                )?);
            }
        }
    }
    anyhow::bail!("not responding to this case")
}

fn to_response(text: &str, responding_to: Value) -> anyhow::Result<Vec<Response>> {
    Ok(vec![Response {
        text: text.to_owned(),
        chat_id: responding_to["message"]["chat"]["id"]
            .as_i64()
            .context("could not get chat id")?,
        reply_to_message_id: None,
    }])
}

fn main() {
    Lazy::force(&TELEGRAM);
    loop {}
}
