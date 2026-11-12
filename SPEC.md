# discord-market-bot — Spec

Discord bot for an EVE Online corp's market channel: a shared list of what people sell and what they want hauled from Jita.

## Scope
- Multi-tenant: one bot instance, invited to many Discord servers; **tenant = Discord server (guild)**
- No ISK handling, no price lookups, no item validation (free text — e.g. "Vexor doctrine fit")
- No expiry, no partial fills, no negotiation

## Order types
| Type | Example | Accepted by |
|---|---|---|
| **Sell** | "Paladin, in hole — 1.2B" | Buyer |
| **Haul** | "2× Vexor doctrine fit from Jita" | Hauler |

Fields:
- `item` — free text (required)
- `qty` — integer, default 1
- `price` — **total** ISK, required for Sell, not used for Haul
- `note` — optional free text

## Flow
1. Member posts an order (Sell or Haul)
2. Another member accepts **one** order and enters their **in-game character name** (asked every time, not remembered)
3. Bot DMs **both parties**: order details, Discord user, character name
4. Order is removed from the list — no other states (no claimed/delivered)

## Rules
- Poster can delete their own open order
- Poster cannot accept their own order
- Accept must be atomic (two people accepting at once → only one wins)

## Multi-tenancy
- Every row (orders, board config) keyed by `guild_id`; every query filtered by it
- Orders, board and `/orders` only ever show the current server's data
- Per-server config set by an admin: `/setup channel:#market` (needs Manage Server) → posts/pins the board there
- Slash commands registered **globally** (new servers work right after invite)
- Bot leaving a server (`GuildDelete`) → keep data (no auto-delete)

## List
- **Board**: pinned, auto-updating message in the market channel, sections **Sell** and **Haul**
- **`/orders`**: same list on demand (ephemeral)

## Tech
- Rust, Discord slash commands / buttons / modals (e.g. poise + serenity)
- SQLite
- Runs on Ben's VPS — Docker Compose, image built on the box from the git checkout
- Config via env: `DISCORD_TOKEN`, `DATABASE_PATH` (optional `DEV_GUILD_ID` for instant command registration while developing)
