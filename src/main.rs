use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    time::Duration,
};

use acidjson::AcidJson;
use anyhow::Context;
use argh::FromArgs;
use once_cell::sync::Lazy;
use rand::{seq::SliceRandom, thread_rng};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use smol_timeout::TimeoutExt;
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
    giftcards: BTreeSet<String>,
    participants: BTreeSet<i64>, // list of all chat ids
    secret_code: Option<String>,
}

async fn send_giftcards() {
    // shuffle participants list
    let mut store = STORE.read().clone();
    let mut participants: Vec<i64> = store.participants.iter().copied().collect();
    participants.shuffle(&mut thread_rng());
    for chat_id in participants {
        if let Some(gc) = store.giftcards.pop_first() {
            let fallible = async {
                TELEGRAM
                    .send_msg(Response {
                        text: "Congratulations! You won a giftcard 🎁 The code is:".into(),
                        chat_id,
                        reply_to_message_id: None,
                    })
                    .timeout(Duration::from_secs(10))
                    .await
                    .context("timeout")??;
                TELEGRAM
                    .send_msg(Response {
                        text: gc,
                        chat_id,
                        reply_to_message_id: None,
                    })
                    .await?;
                anyhow::Ok(())
            };
            if let Err(err) = fallible.await {
                eprintln!("error giving out a giftcard to {chat_id}: {:?}", err);
            } else {
                eprintln!("gave out a giftcard to {chat_id}");
                STORE.write().participants.remove(&chat_id);
                eprintln!("removed {chat_id} from participants");
            }
            smol::Timer::after(Duration::from_millis(200)).await;
        }
    }
    STORE.write().participants.clear();
    STORE.write().giftcards.clear();
}

static STORE: Lazy<AcidJson<Store>> = Lazy::new(|| {
    AcidJson::open_or_else(Path::new(&CONFIG.store_path), || Store {
        giftcards: BTreeSet::new(),
        participants: BTreeSet::new(),
        secret_code: None,
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
    eprintln!("msg = {msg}");
    if update["message"]["chat"]["type"].as_str() == Some("private") {
        let mut username = "";
        if let Some(uname) = update["message"]["from"]["username"].as_str() {
            username = uname;
        };

        if username == admin_uname {
            // start raffle
            if msg.starts_with("#StartRaffle") {
                let mut store = STORE.write();
                store.giftcards.clear();
                let mut lines = msg.split_terminator('\n').skip(1);
                let secret_code = lines.next().filter(|code| code.starts_with("#SecretCode"));
                eprintln!("secret code = {secret_code:?}");
                store.secret_code = secret_code.map(|code| code.replace("#SecretCode ", ""));
                for word in lines {
                    if word.chars().all(|c| c.is_uppercase() || c.is_numeric()) && word.len() > 5 {
                        eprintln!("inserting {word} into giftcard store!");
                        store.giftcards.insert(word.to_string());
                    }
                }
                return to_response("Raffle started", update);
            }
            // end raffle
            else if msg == "#EndRaffle" {
                send_giftcards().await;
                return to_response("Horray! We gave out all the gift cards!", update);
            }
            // display participants count
            else if msg == "#ParticipantsCount" {
                let count = STORE.read().participants.len();
                return to_response(&count.to_string(), update);
            }
            // display giftcards count
            else if msg == "#GiftcardsCount" {
                let count = STORE.read().giftcards.len();
                return to_response(&count.to_string(), update);
            }
        } else if STORE.read().giftcards.is_empty() {
            // no ongoing raffle
            return to_response("Sorry! There's no ongoing raffle at the moment. Watch out for future raffles in our user group!", update);
        } else {
            // exists ongoing raffle
            let chat_id = update["message"]["chat"]["id"]
                .as_i64()
                .context("could not get chat id")?;
            let mut store = STORE.write();
            if let Some(secret_code) = &store.secret_code {
                if !msg.contains(secret_code) {
                    return to_response("⛔ Incorrect secret code! Please provide the correct code to enter the raffle 🔑", update);
                }
            }
            store.participants.insert(chat_id);
            return to_response("🎉 Yay! You've been entered into the raffle!", update);
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
    loop {
        std::thread::park();
    }
}
