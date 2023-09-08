#!/bin/sh
export PATH=$PATH:/root/.cargo/bin

bot_path='/root/telegram-raffle-bot/'
config_path='/root/telegram-raffle-bot/config.yaml'

cd $bot_path
git pull
RUST_LOG=telegram-raffle-bot cargo run --release -- -c $config_path