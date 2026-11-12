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
Env vars, set in `.env` (see `.env.example`):

| Env | |
|---|---|
| `DISCORD_TOKEN` | required |
| `DATABASE_PATH` | required; compose sets it to `/data/market.db` on the `data` volume |
| `DEV_GUILD_ID` | optional; registers commands only in that server (instant) instead of globally |
| `RUST_LOG` | optional, default `info,serenity=warn` |

## Deploy (VPS)
First time:
```sh
git clone https://github.com/bindermuehle/discord-market-bot.git ~/discord-market-bot
cd ~/discord-market-bot
cp .env.example .env && chmod 600 .env   # fill in DISCORD_TOKEN
docker compose up -d --build
```

Update:
```sh
cd ~/discord-market-bot && git pull && docker compose up -d --build
```

Run one instance only. The database is in the `discord-market-bot_data` volume. Backup (safe while running):
```sh
docker compose exec bot sqlite3 /data/market.db ".backup /data/backup.db"
docker compose cp bot:/data/backup.db ~/backups/market-$(date +%F).db
```
