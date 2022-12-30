use anyhow::Context as _;
use async_minecraft_ping::ConnectionConfig;
use serenity::async_trait;
use serenity::builder::CreateEmbed;
use serenity::builder::CreateEmbedFooter;
use serenity::model::gateway::Ready;
use serenity::model::prelude::interaction::Interaction;
use serenity::model::prelude::interaction::InteractionResponseType;
use serenity::model::prelude::GuildId;
use serenity::prelude::*;
use shuttle_secrets::SecretStore;
use tracing::{error, info};

struct Bot {
    discord_guild_id: GuildId,
    mc_server: (String, u16),
}

#[async_trait]
impl EventHandler for Bot {
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("{} is connected!", ready.user.name);

        let commands =
            GuildId::set_application_commands(&self.discord_guild_id, &ctx.http, |commands| {
                commands.create_application_command(|command| {
                    command.name("status").description("Get Server Status")
                })
            })
            .await
            .unwrap();

        info!("Registered commands: {:#?}", commands);
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::ApplicationCommand(command) = interaction {
            let response_content = match command.data.name.as_str() {
                "status" => {
                    match get_server_status(&__self.mc_server.0, __self.mc_server.1).await {
                        Ok(message) => message,
                        Err(err) => {
                            error!(?err, "Error while getting data from the MC server");
                            CreateEmbed::default()
                                .description(err.to_string())
                                .to_owned()
                        }
                    }
                }
                command => unreachable!("Unknown command: {}", command),
            };

            let create_interaction_response =
                command.create_interaction_response(&ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| message.add_embed(response_content))
                });

            if let Err(why) = create_interaction_response.await {
                eprintln!("Cannot respond to slash command: {}", why);
            }
        }
    }
}

#[shuttle_service::main]
async fn serenity(
    #[shuttle_secrets::Secrets] secret_store: SecretStore,
) -> shuttle_service::ShuttleSerenity {
    // Get the discord token set in `Secrets.toml`
    let discord_token = secret_store
        .get("DISCORD_TOKEN")
        .context("'DISCORD_TOKEN' was not found")?;

    let mc_server_addr = secret_store
        .get("MC_SERVER_ADDR")
        .context("'MC_SERVER_ADDR' was not found")?;

    let mc_server_port = secret_store
        .get("MC_SERVER_PORT")
        .and_then(|s| s.parse().ok())
        .context("'MC_SERVER_PORT' was not found")?;

    let discord_guild_id = secret_store
        .get("DISCORD_GUILD_ID")
        .and_then(|s| s.parse().ok())
        .context("'DISCORD_GUILD_ID' was not found")?;

    // Set gateway intents, which decides what events the bot will be notified about.
    // Here we don't need any intents so empty
    let intents = GatewayIntents::empty();

    let client = Client::builder(discord_token, intents)
        .event_handler(Bot {
            mc_server: (mc_server_addr, mc_server_port),
            discord_guild_id: GuildId(discord_guild_id),
        })
        .await
        .expect("Err creating client");

    Ok(client)
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

    // let message = serde_json::json!({
    //     "embeds": [
    //         {
    //             "type": "rich",
    //             "title": desc,
    //             "description": format!("Players ({}/{}):\n{}", playercount.0, playercount.1, players),
    //             "footer": {
    //                 "text": format!("Ping: {} ms", latency.as_millis())
    //             }
    //         }
    //     ]
    // });

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
