# discord-market-bot

Sell and haul orders for an EVE corp market channel. See `SPEC.md`.

## Commands
- `/sell item price [qty] [note]` — price is total ISK, accepts `1.2b`, `350m`, `1500000`
- `/haul item [qty] [note]`
- `/accept order` — asks for your character name, DMs both parties, removes the order
- `/delete order` — your own orders only
- `/orders` — list (only visible to you)
- `/setup channel` — Manage Server only; posts and pins the board

The board also has "Accept an order" and "Delete my order" buttons (select menus show up to 25 orders; the commands cover the rest).

## Discord setup
- Bot scopes: `bot`, `applications.commands`. No privileged intents.
- Permissions in the market channel: View Channel, Send Messages, Embed Links, Read Message History, Pin Messages (or Manage Messages).

## Config
| Env | |
|---|---|
| `DISCORD_TOKEN` | required |
| `DATABASE_PATH` | required, SQLite file (created if missing) |
| `DEV_GUILD_ID` | optional; registers commands only in that server (instant) instead of globally |
| `RUST_LOG` | optional, default `info,serenity=warn` |

## Deploy
```sh
cargo build --release   # build on/for the VPS target (e.g. x86_64-unknown-linux-gnu)
sudo install target/release/discord-market-bot /usr/local/bin/
sudo useradd --system --no-create-home marketbot
echo 'DISCORD_TOKEN=...' | sudo tee /etc/discord-market-bot.env && sudo chmod 600 /etc/discord-market-bot.env
sudo cp deploy/discord-market-bot.service /etc/systemd/system/
sudo systemctl daemon-reload && sudo systemctl enable --now discord-market-bot
```
