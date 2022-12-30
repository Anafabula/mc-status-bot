use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context as _;
use async_minecraft_ping::ConnectionConfig;
use poise::serenity_prelude::CreateEmbed;
use poise::serenity_prelude::CreateEmbedFooter;
use poise::serenity_prelude::GatewayIntents;
use poise::Framework;
use shuttle_secrets::SecretStore;
use tracing::{error, info};

struct Data {
    // User data, which is stored and accessible in all command invocations
    mc_server: (String, u16),
}
type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

struct PoiseService<T, E> {
    framework: Arc<Framework<T, E>>,
}

#[shuttle_service::async_trait]
impl<
        T: std::marker::Send + std::marker::Sync + 'static,
        E: std::marker::Send + std::marker::Sync + 'static,
    > shuttle_service::Service for PoiseService<T, E>
{
    async fn bind(
        mut self: Box<Self>,
        _addr: std::net::SocketAddr,
    ) -> Result<(), shuttle_service::error::Error> {
        self.framework
            .start()
            .await
            .map_err(shuttle_service::error::CustomError::new)?;

        Ok(())
    }
}

#[shuttle_service::main]
async fn serenity(
    #[shuttle_secrets::Secrets] secret_store: SecretStore,
) -> Result<PoiseService<Data, Error>, shuttle_service::Error> {
    let discord_token = secret_store
        .get("DISCORD_TOKEN")
        .context("'DISCORD_TOKEN' was not found")?;

    let mc_server_addr = secret_store
        .get("MC_SERVER_ADDR")
        .context("'MC_SERVER_ADDR' was not found")?;

    let mc_server_port = secret_store
        .get("MC_SERVER_PORT")
        .and_then(|s| s.parse::<u16>().ok())
        .context("'MC_SERVER_PORT' was not found")?;

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![status()],
            ..Default::default()
        })
        .token(discord_token)
        .intents(GatewayIntents::empty())
        .setup(move |ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                Ok(Data {
                    mc_server: (mc_server_addr, mc_server_port),
                })
            })
        })
        .build()
        .await
        .map_err(|err| {
            error!("Error building poise framework: {err}");
            anyhow!(err)
        })?;

    Ok(PoiseService { framework })
}

#[poise::command(slash_command)]
async fn status(ctx: Context<'_>) -> Result<(), Error> {
    let data = ctx.data();
    let embed = match get_server_status(&data.mc_server.0, data.mc_server.1).await {
        Ok(message) => message,
        Err(err) => {
            error!("Error while getting data from the MC server: {err}");
            CreateEmbed::default()
                .description(err.to_string())
                .to_owned()
        }
    };
    info!("Replying with embed: {embed:?}");
    ctx.send(|m| {
        m.embeds.push(embed);
        m
    })
    .await?;
    Ok(())
}

async fn get_server_status(
    addr: &str,
    port: u16,
) -> Result<CreateEmbed, async_minecraft_ping::ServerError> {
    let config = ConnectionConfig::build(addr).with_port(port);

    let connection = config.connect().await?;

    let connection = connection.status().await?;

    let players = if let Some(players) = &connection.status.players.sample {
        players
            .iter()
            .map(|p| p.name.clone())
            .collect::<Vec<String>>()
            .join("\n")
    } else {
        "".to_owned()
    };

    let desc = match connection.status.description {
        async_minecraft_ping::ServerDescription::Plain(ref desc) => desc,
        async_minecraft_ping::ServerDescription::Object { ref text } => text,
    }
    .to_owned();

    let playercount = (
        connection.status.players.online,
        connection.status.players.max,
    );

    let start = tokio::time::Instant::now();
    connection.ping(299792458).await?;
    let latency = start.elapsed();

    let mut embed = CreateEmbed::default();

    embed
        .title(desc)
        .description(format!(
            "Players ({}/{}):\n{}",
            playercount.0, playercount.1, players
        ))
        .set_footer(
            CreateEmbedFooter::default()
                .text(format!("Ping: {} ms", latency.as_millis()))
                .to_owned(),
        );

    Ok(embed)
}
