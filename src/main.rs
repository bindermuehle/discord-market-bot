mod accept;
mod board;
mod commands;
mod db;
mod isk;

use anyhow::Context as _;
use poise::serenity_prelude as serenity;

pub struct Data {
    pub db: db::Db,
    /// Held while editing or reposting a board, so concurrent refreshes can't post two boards.
    pub board_lock: tokio::sync::Mutex<()>,
}

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;

async fn on_error(error: poise::FrameworkError<'_, Data, Error>) {
    match error {
        poise::FrameworkError::Command { error, ctx, .. } => {
            tracing::error!(command = ctx.command().name, "command failed: {error}");
            let _ = ctx
                .send(
                    poise::CreateReply::default()
                        .content("Something went wrong. Try again in a moment.")
                        .ephemeral(true),
                )
                .await;
        }
        other => {
            if let Err(e) = poise::builtins::on_error(other).await {
                tracing::error!("error while handling error: {e}");
            }
        }
    }
}

async fn event_handler(
    ctx: &serenity::Context,
    event: &serenity::FullEvent,
    _framework: poise::FrameworkContext<'_, Data, Error>,
    data: &Data,
) -> Result<(), Error> {
    match event {
        serenity::FullEvent::InteractionCreate { interaction } => {
            let result = match interaction {
                serenity::Interaction::Component(i) => accept::handle_component(ctx, data, i).await,
                serenity::Interaction::Modal(m) => accept::handle_modal(ctx, data, m).await,
                _ => Ok(()),
            };
            if let Err(e) = result {
                tracing::error!("interaction failed: {e}");
            }
        }
        serenity::FullEvent::GuildDelete { incomplete, .. } => {
            // Data is kept on purpose; the bot may be re-invited.
            tracing::info!(guild_id = %incomplete.id, "removed from guild (data kept)");
        }
        _ => {}
    }
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,serenity=warn".into()),
        )
        .init();

    let token = std::env::var("DISCORD_TOKEN").context("DISCORD_TOKEN is not set")?;
    let db_path = std::env::var("DATABASE_PATH").context("DATABASE_PATH is not set")?;
    let dev_guild = match std::env::var("DEV_GUILD_ID") {
        Ok(s) if !s.trim().is_empty() => Some(serenity::GuildId::new(
            s.trim().parse().context("DEV_GUILD_ID must be a numeric guild id")?,
        )),
        _ => None,
    };

    let db = db::Db::open(&db_path).with_context(|| format!("opening database {db_path}"))?;

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: commands::all(),
            on_error: |e| Box::pin(on_error(e)),
            event_handler: |ctx, event, framework, data| {
                Box::pin(event_handler(ctx, event, framework, data))
            },
            ..Default::default()
        })
        .setup(move |ctx, ready, framework| {
            Box::pin(async move {
                let cmds = &framework.options().commands;
                match dev_guild {
                    Some(g) => {
                        poise::builtins::register_in_guild(ctx, cmds, g).await?;
                        tracing::info!(guild_id = %g, "registered commands in dev guild");
                    }
                    None => {
                        poise::builtins::register_globally(ctx, cmds).await?;
                        tracing::info!("registered commands globally");
                    }
                }
                tracing::info!(user = %ready.user.name, guilds = ready.guilds.len(), "ready");
                Ok(Data { db, board_lock: tokio::sync::Mutex::new(()) })
            })
        })
        .build();

    let mut client = serenity::ClientBuilder::new(token, serenity::GatewayIntents::GUILDS)
        .framework(framework)
        .await
        .context("building Discord client")?;
    client.start().await.context("Discord client stopped")?;
    Ok(())
}
