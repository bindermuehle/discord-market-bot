//! Rendering of the order list and keeping the pinned board message current.

use poise::serenity_prelude as serenity;
use serenity::{
    ChannelId, CreateActionRow, CreateButton, CreateEmbed, CreateEmbedFooter, CreateMessage,
    EditMessage, GuildId, MessageId,
};

use crate::db::{Board, Kind, Order};
use crate::{Data, isk};

pub const BUTTON_ACCEPT: &str = "market:accept";
pub const BUTTON_DELETE: &str = "market:delete";

/// Space per section; two sections plus headers stay under the 4096-char embed description limit.
const SECTION_BUDGET: usize = 1900;

/// Escapes Discord markdown in user-supplied text so it shows literally: no masked links
/// (`[text](url)`), no mention or channel syntax (`<@id>`), no formatting.
pub fn escape_md(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '*' | '_' | '`' | '~' | '|' | '>' | '<' | '[' | ']' | '(' | ')' | '#' | '-' | ':' | '@') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

pub fn order_line(o: &Order) -> String {
    let mut line = format!("`#{}` ", o.id);
    if o.qty != 1 {
        line.push_str(&format!("{}× ", o.qty));
    }
    line.push_str(&escape_md(&o.item));
    if let Some(price) = o.price {
        line.push_str(&format!(" — **{}**", isk::format(price)));
    }
    line.push_str(&format!(" · <@{}>", o.poster_id));
    if let Some(note) = &o.note {
        line.push_str(&format!(" · _{}_", escape_md(note)));
    }
    line
}

fn section(orders: &[Order], kind: Kind) -> String {
    let items: Vec<&Order> = orders.iter().filter(|o| o.kind == kind).collect();
    if items.is_empty() {
        return "_none_".to_string();
    }
    let mut out = String::new();
    for (i, o) in items.iter().enumerate() {
        let line = order_line(o);
        if out.len() + line.len() + 40 > SECTION_BUDGET {
            out.push_str(&format!("…and {} more (see `/orders`)", items.len() - i));
            break;
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}

pub fn render(orders: &[Order]) -> CreateEmbed {
    let description = format!(
        "## Sell\n{}\n## Haul\n{}",
        section(orders, Kind::Sell),
        section(orders, Kind::Haul)
    );
    let description: String = description.chars().take(4096).collect();
    CreateEmbed::new()
        .title("Market board")
        .description(description)
        .footer(CreateEmbedFooter::new(
            "Post with /sell or /haul · accept with /accept or the button below",
        ))
        .timestamp(serenity::Timestamp::now())
}

fn buttons() -> Vec<CreateActionRow> {
    vec![CreateActionRow::Buttons(vec![
        CreateButton::new(BUTTON_ACCEPT)
            .label("Accept an order")
            .style(serenity::ButtonStyle::Primary),
        CreateButton::new(BUTTON_DELETE)
            .label("Delete my order")
            .style(serenity::ButtonStyle::Secondary),
    ])]
}

/// Posts a new board message in `channel_id` and tries to pin it.
/// Returns the message id and whether pinning worked.
pub async fn post(
    http: &serenity::Http,
    data: &Data,
    guild_id: GuildId,
    channel_id: ChannelId,
) -> anyhow::Result<(MessageId, bool)> {
    let orders = data.db.list_orders(guild_id.get())?;
    let msg = channel_id
        .send_message(
            http,
            CreateMessage::new().embed(render(&orders)).components(buttons()),
        )
        .await?;
    let pinned = match msg.pin(http).await {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!(%guild_id, %channel_id, "pinning board failed: {e}");
            false
        }
    };
    Ok((msg.id, pinned))
}

/// Re-renders the board for a guild. Reposts it if the message was deleted.
/// Errors are logged, not returned: a stale board must not fail the user's action.
pub async fn refresh(http: &serenity::Http, data: &Data, guild_id: GuildId) {
    if let Err(e) = try_refresh(http, data, guild_id).await {
        tracing::warn!(%guild_id, "board refresh failed: {e:#}");
    }
}

async fn try_refresh(http: &serenity::Http, data: &Data, guild_id: GuildId) -> anyhow::Result<()> {
    // Serialise refreshes so two concurrent repost attempts don't create two boards.
    let _guard = data.board_lock.lock().await;
    let Some(board) = data.db.get_board(guild_id.get())? else {
        return Ok(());
    };
    let channel_id = ChannelId::new(board.channel_id);
    if let Some(message_id) = board.message_id {
        let orders = data.db.list_orders(guild_id.get())?;
        let edit = EditMessage::new().embed(render(&orders)).components(buttons());
        match channel_id.edit_message(http, MessageId::new(message_id), edit).await {
            Ok(_) => return Ok(()),
            Err(serenity::Error::Http(e))
                if e.status_code() == Some(serenity::StatusCode::NOT_FOUND) =>
            {
                tracing::info!(%guild_id, "board message gone, reposting");
            }
            Err(e) => return Err(e.into()),
        }
    }
    let (message_id, _) = post(http, data, guild_id, channel_id).await?;
    data.db.set_board(
        guild_id.get(),
        Board { channel_id: board.channel_id, message_id: Some(message_id.get()) },
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_links_and_mentions() {
        assert_eq!(escape_md("[x](http://a)"), r"\[x\]\(http\://a\)");
        assert_eq!(escape_md("<@123>"), r"\<\@123\>");
        assert_eq!(escape_md("Vexor fit"), "Vexor fit");
    }
}
