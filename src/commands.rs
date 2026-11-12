//! Slash commands. All are guild-only; every DB call is scoped to the invoking guild.

use poise::serenity_prelude as serenity;
use serenity::{AutocompleteChoice, ChannelId, CreateInteractionResponse, GuildId};

use crate::db::{Board, Kind, NewOrder, Order};
use crate::{Context, Error, accept, board, isk};

fn guild(ctx: Context<'_>) -> Result<GuildId, Error> {
    ctx.guild_id().ok_or_else(|| "this command only works in a server".into())
}

async fn reply_ephemeral(ctx: Context<'_>, text: impl Into<String>) -> Result<(), Error> {
    ctx.send(poise::CreateReply::default().content(text).ephemeral(true)).await?;
    Ok(())
}

fn clean(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

async fn post_order(
    ctx: Context<'_>,
    kind: Kind,
    item: String,
    qty: Option<i64>,
    price: Option<i64>,
    note: Option<String>,
) -> Result<(), Error> {
    let guild_id = guild(ctx)?;
    let item = item.trim();
    if item.is_empty() {
        return reply_ephemeral(ctx, "Item can't be empty.").await;
    }
    let note = clean(note);
    let id = ctx.data().db.insert_order(&NewOrder {
        guild_id: guild_id.get(),
        kind,
        item,
        qty: qty.unwrap_or(1),
        price,
        note: note.as_deref(),
        poster_id: ctx.author().id.get(),
    })?;
    let has_board = ctx.data().db.get_board(guild_id.get())?.is_some();
    let mut msg = format!("Posted {} order `#{id}`.", kind.label());
    if !has_board {
        msg.push_str(" No board is set up on this server yet; an admin can run `/setup`.");
    }
    reply_ephemeral(ctx, msg).await?;
    board::refresh(ctx.http(), ctx.data(), guild_id).await;
    Ok(())
}

/// Post an item for sale.
#[poise::command(slash_command, guild_only)]
pub async fn sell(
    ctx: Context<'_>,
    #[description = "What you sell, free text (e.g. \"Paladin, in hole\")"]
    #[max_length = 200]
    item: String,
    #[description = "Total price in ISK (e.g. 1.2b, 350m, 1500000)"]
    #[max_length = 30]
    price: String,
    #[description = "Quantity (default 1)"]
    #[min = 1]
    #[max = 1000000]
    qty: Option<i64>,
    #[description = "Optional note"]
    #[max_length = 300]
    note: Option<String>,
) -> Result<(), Error> {
    let Some(price) = isk::parse(&price) else {
        return reply_ephemeral(
            ctx,
            format!("Couldn't read `{price}` as an ISK amount. Use e.g. `1.2b`, `350m` or `1500000`."),
        )
        .await;
    };
    post_order(ctx, Kind::Sell, item, qty, Some(price), note).await
}

/// Request something to be hauled from Jita.
#[poise::command(slash_command, guild_only)]
pub async fn haul(
    ctx: Context<'_>,
    #[description = "What to haul, free text (e.g. \"Vexor doctrine fit\")"]
    #[max_length = 200]
    item: String,
    #[description = "Quantity (default 1)"]
    #[min = 1]
    #[max = 1000000]
    qty: Option<i64>,
    #[description = "Optional note"]
    #[max_length = 300]
    note: Option<String>,
) -> Result<(), Error> {
    post_order(ctx, Kind::Haul, item, qty, None, note).await
}

/// Show all open orders on this server (only visible to you).
#[poise::command(slash_command, guild_only)]
pub async fn orders(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = guild(ctx)?;
    let orders = ctx.data().db.list_orders(guild_id.get())?;
    ctx.send(
        poise::CreateReply::default()
            .embed(board::render(&orders))
            .ephemeral(true),
    )
    .await?;
    Ok(())
}

fn matching_choices(
    orders: Vec<Order>,
    partial: &str,
    keep: impl Fn(&Order) -> bool,
) -> Vec<AutocompleteChoice> {
    let partial = partial.trim().trim_start_matches('#').to_lowercase();
    orders
        .into_iter()
        .filter(|o| keep(o))
        .filter(|o| {
            partial.is_empty()
                || o.id.to_string().starts_with(&partial)
                || o.item.to_lowercase().contains(&partial)
        })
        .take(25)
        .map(|o| AutocompleteChoice::new(accept::option_label(&o), o.id))
        .collect()
}

async fn autocomplete_acceptable(ctx: Context<'_>, partial: &str) -> Vec<AutocompleteChoice> {
    let (Some(guild_id), me) = (ctx.guild_id(), ctx.author().id.get()) else {
        return vec![];
    };
    let orders = ctx.data().db.list_orders(guild_id.get()).unwrap_or_default();
    matching_choices(orders, partial, |o| o.poster_id != me)
}

async fn autocomplete_own(ctx: Context<'_>, partial: &str) -> Vec<AutocompleteChoice> {
    let (Some(guild_id), me) = (ctx.guild_id(), ctx.author().id.get()) else {
        return vec![];
    };
    let orders = ctx.data().db.list_orders(guild_id.get()).unwrap_or_default();
    matching_choices(orders, partial, |o| o.poster_id == me)
}

/// Accept an order. You'll be asked for your in-game character name.
#[poise::command(slash_command, guild_only)]
pub async fn accept(
    ctx: poise::ApplicationContext<'_, crate::Data, Error>,
    #[description = "Order to accept"]
    #[autocomplete = "autocomplete_acceptable"]
    order: i64,
) -> Result<(), Error> {
    let actx = Context::Application(ctx);
    let guild_id = guild(actx)?;
    let Some(o) = ctx.data().db.get_order(guild_id.get(), order)? else {
        return reply_ephemeral(actx, format!("Order `#{order}` doesn't exist or was already taken.")).await;
    };
    if o.poster_id == ctx.author().id.get() {
        return reply_ephemeral(actx, "You can't accept your own order.").await;
    }
    ctx.interaction
        .create_response(ctx.http(), CreateInteractionResponse::Modal(accept::modal(&o)))
        .await?;
    ctx.has_sent_initial_response
        .store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

/// Delete one of your own open orders.
#[poise::command(slash_command, guild_only)]
pub async fn delete(
    ctx: Context<'_>,
    #[description = "Your order to delete"]
    #[autocomplete = "autocomplete_own"]
    order: i64,
) -> Result<(), Error> {
    let guild_id = guild(ctx)?;
    match ctx.data().db.delete_own_order(guild_id.get(), order, ctx.author().id.get())? {
        Some(o) => {
            reply_ephemeral(ctx, format!("Deleted `#{}` ({}).", o.id, o.item)).await?;
            board::refresh(ctx.http(), ctx.data(), guild_id).await;
            Ok(())
        }
        None => reply_ephemeral(ctx, format!("`#{order}` isn't one of your open orders.")).await,
    }
}

/// Set the market channel and post the pinned board there (admins only).
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD"
)]
pub async fn setup(
    ctx: Context<'_>,
    #[description = "Channel for the market board"]
    #[channel_types("Text")]
    channel: serenity::GuildChannel,
) -> Result<(), Error> {
    let guild_id = guild(ctx)?;
    if channel.guild_id != guild_id {
        return reply_ephemeral(ctx, "That channel isn't on this server.").await;
    }
    ctx.defer_ephemeral().await?;
    let data = ctx.data();
    let http = ctx.http();
    let _guard = data.board_lock.lock().await;

    // Remove the previous board, if any (best effort).
    if let Some(Board { channel_id, message_id: Some(message_id) }) = data.db.get_board(guild_id.get())? {
        let old = ChannelId::new(channel_id);
        let _ = old.delete_message(http, serenity::MessageId::new(message_id)).await;
    }

    let (message_id, pinned) = match board::post(http, data, guild_id, channel.id).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(%guild_id, "setup: posting board failed: {e:#}");
            ctx.say(format!(
                "Couldn't post in <#{}>. The bot needs View Channel, Send Messages and Embed Links there.",
                channel.id
            ))
            .await?;
            return Ok(());
        }
    };
    data.db.set_board(
        guild_id.get(),
        Board { channel_id: channel.id.get(), message_id: Some(message_id.get()) },
    )?;
    let mut msg = format!("Market board posted in <#{}>.", channel.id);
    if !pinned {
        msg.push_str(" Pinning failed: give the bot the Pin Messages (or Manage Messages) permission there, then pin it by hand or re-run `/setup`.");
    }
    ctx.say(msg).await?;
    Ok(())
}

pub fn all() -> Vec<poise::Command<crate::Data, Error>> {
    vec![sell(), haul(), orders(), accept(), delete(), setup()]
}
