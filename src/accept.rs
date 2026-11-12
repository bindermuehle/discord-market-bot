//! Button, select-menu and modal handling: the accept and delete flows started from the board,
//! and the character-name modal shared with `/accept`.

use poise::serenity_prelude as serenity;
use serenity::{
    ActionRowComponent, ComponentInteraction, ComponentInteractionDataKind, CreateActionRow,
    CreateEmbed, CreateInputText, CreateInteractionResponse, CreateInteractionResponseMessage,
    CreateMessage, CreateModal, CreateSelectMenu, CreateSelectMenuKind, CreateSelectMenuOption,
    EditInteractionResponse, InputTextStyle, ModalInteraction, User, UserId,
};

use crate::db::{AcceptOutcome, Kind, Order};
use crate::{Data, Error, board, isk};

const SELECT_ACCEPT: &str = "market:accept_select";
const SELECT_DELETE: &str = "market:delete_select";
const MODAL_PREFIX: &str = "market:accept_modal:";
const INPUT_CHARACTER: &str = "character";

pub fn modal(o: &Order) -> CreateModal {
    let title: String = format!("Accept #{}: {}", o.id, o.item).chars().take(45).collect();
    CreateModal::new(format!("{MODAL_PREFIX}{}", o.id), title).components(vec![
        CreateActionRow::InputText(
            CreateInputText::new(InputTextStyle::Short, "Your in-game character name", INPUT_CHARACTER)
                .min_length(3)
                .max_length(37)
                .required(true),
        ),
    ])
}

pub fn option_label(o: &Order) -> String {
    let mut s = format!("#{} {}: ", o.id, o.kind.label());
    if o.qty != 1 {
        s.push_str(&format!("{}× ", o.qty));
    }
    s.push_str(&o.item);
    if let Some(p) = o.price {
        s.push_str(&format!(" — {}", isk::format(p)));
    }
    s.chars().take(100).collect()
}

fn select_menu(custom_id: &str, placeholder: &str, orders: &[Order]) -> CreateActionRow {
    let options = orders
        .iter()
        .take(25)
        .map(|o| {
            let mut opt = CreateSelectMenuOption::new(option_label(o), o.id.to_string());
            if let Some(note) = &o.note {
                opt = opt.description(note.chars().take(100).collect::<String>());
            }
            opt
        })
        .collect();
    CreateActionRow::SelectMenu(
        CreateSelectMenu::new(custom_id, CreateSelectMenuKind::String { options })
            .placeholder(placeholder),
    )
}

async fn ephemeral(
    http: &serenity::Http,
    i: &ComponentInteraction,
    msg: CreateInteractionResponseMessage,
) -> Result<(), Error> {
    i.create_response(http, CreateInteractionResponse::Message(msg.ephemeral(true)))
        .await?;
    Ok(())
}

pub async fn handle_component(
    ctx: &serenity::Context,
    data: &Data,
    i: &ComponentInteraction,
) -> Result<(), Error> {
    let Some(guild_id) = i.guild_id else { return Ok(()) };
    let me = i.user.id.get();
    let http = &ctx.http;

    match i.data.custom_id.as_str() {
        board::BUTTON_ACCEPT => {
            let orders: Vec<Order> = data
                .db
                .list_orders(guild_id.get())?
                .into_iter()
                .filter(|o| o.poster_id != me)
                .collect();
            let msg = if orders.is_empty() {
                CreateInteractionResponseMessage::new().content("There are no orders you can accept.")
            } else {
                let mut text = "Pick the order to accept:".to_string();
                if orders.len() > 25 {
                    text.push_str(&format!(
                        "\nShowing the oldest 25 of {}. Use `/accept` to find any order.",
                        orders.len()
                    ));
                }
                CreateInteractionResponseMessage::new()
                    .content(text)
                    .components(vec![select_menu(SELECT_ACCEPT, "Choose an order", &orders)])
            };
            ephemeral(http, i, msg).await
        }
        board::BUTTON_DELETE => {
            let orders: Vec<Order> = data
                .db
                .list_orders(guild_id.get())?
                .into_iter()
                .filter(|o| o.poster_id == me)
                .collect();
            let msg = if orders.is_empty() {
                CreateInteractionResponseMessage::new().content("You have no open orders.")
            } else {
                let mut text = "Pick the order to delete:".to_string();
                if orders.len() > 25 {
                    text.push_str("\nShowing your oldest 25. Use `/delete` for the others.");
                }
                CreateInteractionResponseMessage::new()
                    .content(text)
                    .components(vec![select_menu(SELECT_DELETE, "Choose your order", &orders)])
            };
            ephemeral(http, i, msg).await
        }
        SELECT_ACCEPT => {
            let Some(id) = selected_id(i) else { return Ok(()) };
            match data.db.get_order(guild_id.get(), id)? {
                None => ephemeral(http, i, CreateInteractionResponseMessage::new()
                    .content(format!("Order `#{id}` was already taken or deleted."))).await,
                Some(o) if o.poster_id == me => ephemeral(http, i, CreateInteractionResponseMessage::new()
                    .content("You can't accept your own order.")).await,
                Some(o) => {
                    i.create_response(http, CreateInteractionResponse::Modal(modal(&o))).await?;
                    Ok(())
                }
            }
        }
        SELECT_DELETE => {
            let Some(id) = selected_id(i) else { return Ok(()) };
            let text = match data.db.delete_own_order(guild_id.get(), id, me)? {
                Some(o) => format!("Deleted `#{}` ({}).", o.id, board::escape_md(&o.item)),
                None => format!("`#{id}` isn't one of your open orders anymore."),
            };
            i.create_response(
                http,
                CreateInteractionResponse::UpdateMessage(
                    CreateInteractionResponseMessage::new().content(text).components(vec![]),
                ),
            )
            .await?;
            board::refresh(http, data, guild_id).await;
            Ok(())
        }
        _ => Ok(()),
    }
}

fn selected_id(i: &ComponentInteraction) -> Option<i64> {
    match &i.data.kind {
        ComponentInteractionDataKind::StringSelect { values } => values.first()?.parse().ok(),
        _ => None,
    }
}

pub async fn handle_modal(
    ctx: &serenity::Context,
    data: &Data,
    m: &ModalInteraction,
) -> Result<(), Error> {
    let Some(id) = m.data.custom_id.strip_prefix(MODAL_PREFIX) else { return Ok(()) };
    let Ok(id) = id.parse::<i64>() else { return Ok(()) };
    let Some(guild_id) = m.guild_id else { return Ok(()) };
    let http = &ctx.http;

    let character = m
        .data
        .components
        .iter()
        .flat_map(|row| &row.components)
        .find_map(|c| match c {
            ActionRowComponent::InputText(t) if t.custom_id == INPUT_CHARACTER => t.value.clone(),
            _ => None,
        })
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    m.defer_ephemeral(http).await?;
    let reply = |text: String| EditInteractionResponse::new().content(text);

    if character.is_empty() {
        m.edit_response(http, reply("Character name can't be empty.".into())).await?;
        return Ok(());
    }

    let order = match data.db.accept_order(guild_id.get(), id, m.user.id.get())? {
        AcceptOutcome::Accepted(o) => o,
        AcceptOutcome::Gone => {
            m.edit_response(http, reply(format!("Too late: order `#{id}` was already taken or deleted.")))
                .await?;
            return Ok(());
        }
        AcceptOutcome::OwnOrder => {
            m.edit_response(http, reply("You can't accept your own order.".into())).await?;
            return Ok(());
        }
    };

    board::refresh(http, data, guild_id).await;

    let poster = UserId::new(order.poster_id).to_user(http).await.ok();
    let guild_name = guild_id.name(&ctx.cache).unwrap_or_else(|| "your server".into());
    let embed = dm_embed(&order, poster.as_ref(), &m.user, &character, &guild_name);

    let poster_dm = UserId::new(order.poster_id)
        .direct_message(http, CreateMessage::new()
            .content(format!("Your order `#{}` on **{guild_name}** was accepted.", order.id))
            .embed(embed.clone()))
        .await;
    let acceptor_dm = m
        .user
        .id
        .direct_message(http, CreateMessage::new()
            .content(format!("You accepted order `#{}` on **{guild_name}**.", order.id))
            .embed(embed))
        .await;

    let mut text = format!("Accepted `#{}`. ", order.id);
    match (&poster_dm, &acceptor_dm) {
        (Ok(_), Ok(_)) => text.push_str("You and the poster got a DM with the details."),
        (Err(_), Ok(_)) => text.push_str(&format!(
            "Couldn't DM the poster (DMs closed). Contact <@{}> yourself; your DM has the details.",
            order.poster_id
        )),
        (Ok(_), Err(_)) => text.push_str(&format!(
            "The poster got a DM, but I couldn't DM you (DMs closed). Poster: <@{}>.",
            order.poster_id
        )),
        (Err(_), Err(_)) => text.push_str(&format!(
            "Couldn't DM either of you. Contact <@{}> yourself.",
            order.poster_id
        )),
    }
    if let Err(e) = &poster_dm {
        tracing::info!(%guild_id, order = order.id, "DM to poster failed: {e}");
    }
    if let Err(e) = &acceptor_dm {
        tracing::info!(%guild_id, order = order.id, "DM to acceptor failed: {e}");
    }
    m.edit_response(http, reply(text)).await?;
    Ok(())
}

fn user_line(id: u64, user: Option<&User>) -> String {
    match user {
        Some(u) => format!("<@{id}> ({})", u.name),
        None => format!("<@{id}>"),
    }
}

fn dm_embed(order: &Order, poster: Option<&User>, acceptor: &User, character: &str, guild: &str) -> CreateEmbed {
    let (title, role) = match order.kind {
        Kind::Sell => (format!("Sell order #{} accepted", order.id), "Buyer"),
        Kind::Haul => (format!("Haul order #{} accepted", order.id), "Hauler"),
    };
    let mut e = CreateEmbed::new()
        .title(title)
        .field("Server", guild, true)
        .field("Item", board::escape_md(&order.item), false)
        .field("Quantity", order.qty.to_string(), true);
    if let Some(p) = order.price {
        e = e.field("Total price", isk::format(p), true);
    }
    if let Some(note) = &order.note {
        e = e.field("Note", board::escape_md(note), false);
    }
    e.field("Posted by", user_line(order.poster_id, poster), false)
        .field(role, user_line(acceptor.id.get(), Some(acceptor)), true)
        .field(format!("{role} character"), character, true)
}
