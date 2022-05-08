use futures::StreamExt;
use regex::Regex;
use songbird::{
    input::{ytdl, Input},
    tracks::{PlayMode, TrackHandle},
    Songbird,
};
use std::{
    collections::HashMap,
    env,
    error::Error,
    fs::{File, OpenOptions},
    future::Future,
    path::Path,
    process,
    sync::{Arc, Mutex},
    time::Duration,
};
use std::{fs::read_to_string, io::prelude::*};

use std::io::{BufRead, BufReader};
use tokio::sync::RwLock;
use twilight_cache_inmemory::{InMemoryCache, ResourceType};

use twilight_gateway::{cluster::ShardScheme, Cluster, Event, Intents};
use twilight_http::Client as HttpClient;
use twilight_model::{
    channel::Message,
    gateway::payload::incoming::MessageCreate,
    id::{marker::GuildMarker, Id},
};
use twilight_standby::Standby;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder};

mod yt_utils;

type State = Arc<StateRef>;

#[derive(Debug)]
struct StateRef {
    http: HttpClient,
    trackdata: RwLock<HashMap<Id<GuildMarker>, TrackHandle>>,
    songbird: Songbird,
    standby: Standby,
    cache: InMemoryCache,
}

struct StateInfo {
    is_joined: bool,
    current_song_desc: String,
    current_song_link: String,
}

impl StateInfo {
    pub fn set_is_joined(&mut self, value: bool) {
        self.is_joined = value;
    }
    pub fn set_current_song_desc(&mut self, value: String) {
        self.current_song_desc = value;
    }
    pub fn set_current_song_link(&mut self, value: String) {
        self.current_song_link = value;
    }
}

fn spawn(
    fut: impl Future<Output = Result<(), Box<dyn Error + Send + Sync + 'static>>> + Send + 'static,
) {
    tokio::spawn(async move {
        if let Err(why) = fut.await {
            println!("{}", &why);
        }
    });
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    let (mut events, state, state_info) = {
        let token = get_discord_token();
        if token.len() < 30 {
            println!("{:?} - {}", token, "Is not valid token !");
            process::exit(0x0100);
        }
        env::set_var("DISCORD_TOKEN", token);
        let token = env::var("DISCORD_TOKEN")?;
        let http = HttpClient::new(token.clone());
        let user_id = http.current_user().exec().await?.model().await?.id;

        let scheme = ShardScheme::Auto;

        let intents = Intents::GUILD_MESSAGES
            | Intents::DIRECT_MESSAGES
            | Intents::GUILD_MEMBERS
            | Intents::GUILDS
            | Intents::GUILD_VOICE_STATES
            | Intents::MESSAGE_CONTENT;
        let (cluster, events) = Cluster::builder(token, intents)
            .shard_scheme(scheme)
            .build()
            .await?;

        let thi = tokio::spawn(async move {
            cluster.up().await;
            return Songbird::twilight(Arc::new(cluster), user_id);
        });
        let songbird = thi.await?;
        (
            events,
            Arc::new(StateRef {
                http,
                trackdata: Default::default(),
                songbird,
                standby: Standby::new(),
                cache: InMemoryCache::builder()
                    .resource_types(ResourceType::all())
                    .build(),
            }),
            Arc::new(Mutex::new(StateInfo {
                is_joined: false,
                current_song_desc: String::default(),
                current_song_link: String::default(),
            })),
        )
    };

    while let Some((_, event)) = events.next().await {
        state.standby.process(&event);
        state.cache.update(&event);
        state.songbird.process(&event).await;

        if let Event::MessageCreate(msg) = event {
            if msg.guild_id.is_none() || !msg.content.starts_with('!') {
                continue;
            }

            match msg.content.splitn(2, ' ').next() {
                Some("!join") => spawn(join(msg.0, Arc::clone(&state), Arc::clone(&state_info))),
                Some("!leave") => spawn(leave(msg.0, Arc::clone(&state), Arc::clone(&state_info))),
                Some("!pause") => spawn(pause(msg.0, Arc::clone(&state), Arc::clone(&state_info))),
                Some("!play") => spawn(play(msg.0, Arc::clone(&state), Arc::clone(&state_info))),
                Some("!help") => spawn(help(msg.0, Arc::clone(&state), Arc::clone(&state_info))),
                Some("!stop") => spawn(stop(msg.0, Arc::clone(&state), Arc::clone(&state_info))),
                Some("!time") => spawn(time(msg.0, Arc::clone(&state), Arc::clone(&state_info))),
                Some("!desc") => spawn(description(
                    msg.0,
                    Arc::clone(&state),
                    Arc::clone(&state_info),
                )),
                Some("!volume") => {
                    spawn(volume(msg.0, Arc::clone(&state), Arc::clone(&state_info)))
                }
                _ => continue,
            }
        }
    }

    Ok(())
}

async fn join(
    msg: Message,
    state: State,
    state_info: Arc<Mutex<StateInfo>>,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    let guild_id = msg.guild_id.ok_or("Can't join a non-guild channel.")?;
    let user_id = msg.author.id;

    let channel_to_join: Option<u64>;

    let user_ch = state.cache.voice_state(user_id, guild_id.clone());
    match user_ch {
        Some(test01) => {
            channel_to_join = Some(test01.channel_id().get());
        }
        None => {
            state
                .http
                .create_message(msg.channel_id)
                .content("You're not in a voice channel?")?
                .exec()
                .await?;
            return Ok(());
        }
    }

    let (_handle, success) = state
        .songbird
        .join(guild_id, channel_to_join.unwrap_or_default())
        .await;

    let content: String = match success {
        Ok(()) => {
            state_info.lock().unwrap().set_is_joined(true);
            format!("Joined <#{}>!", channel_to_join.unwrap_or_default())
        }

        Err(e) => format!(
            "Failed to join <#{}>! Why: {:?}",
            channel_to_join.unwrap_or_default(),
            e
        ),
    };
    state
        .http
        .create_message(msg.channel_id)
        .content(&content)?
        .exec()
        .await?;

    Ok(())
}

async fn leave(
    msg: Message,
    state: State,
    state_info: Arc<Mutex<StateInfo>>,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    let guild_id = msg.guild_id.unwrap();
    state_info.lock().unwrap().set_is_joined(false);
    state.songbird.leave(guild_id).await?;

    state
        .http
        .create_message(msg.channel_id)
        .content("Left the channel")?
        .exec()
        .await?;

    Ok(())
}

async fn play(
    msg: Message,
    state: State,
    state_info: Arc<Mutex<StateInfo>>,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    if !state_info.lock().unwrap().is_joined {
        let res = join(msg.clone(), state.clone(), state_info.clone())
            .await
            .ok();

        match res {
            Some(result) => println!("{:?}", result),
            None => println!("ERR"),
        }
    }
    if state_info.lock().unwrap().is_joined {
        let b = msg.content.clone();
        println!("{}", b);
        let index1 = b.find(" ");
        let mut text: String;
        if !index1.is_none() {
            text = b.chars().skip(index1.unwrap()).collect();
        } else {
            text = "".to_string();
        }
        text = text.trim().replace(" ", "+");

        let re = Regex::new(r"^(http(s)://)?((w){3}.)?youtu(be|.be)?(.com)?/.+").unwrap();
        let re2 = Regex::new("^(http://)(.+)").unwrap();
        let mut yt_link: String = String::from("https://www.youtube.com/watch?v=");
        if re.is_match(&text) | re2.is_match(&text) {
            yt_link = text.to_string().to_string();
        } else if text.len() < 1 {
            yt_link = String::from("http://astreaming.virginradio.ro:8000/virgin_aacp_64k");
        } else {
            let mut search_str: String =
                String::from("https://www.youtube.com/results?search_query=");
            search_str.push_str(&text);

            let content = reqwest::get(search_str.to_string()).await?.text().await?;
            yt_link.push_str(
                &yt_utils::extract_links(content.as_str())
                    .iter()
                    //  .skip(1)
                    .next()
                    .unwrap()
                    .to_string(),
            );
            state_info
                .lock()
                .unwrap()
                .set_current_song_link(yt_link.clone());
        }

        let guild_id = msg.guild_id.unwrap();

        if let Ok(song) = ytdl(yt_link).await {
            let input = Input::from(song);

            let content = format!(
                "Playing **{:?}** by **{:?}**",
                input
                    .metadata
                    .title
                    .as_ref()
                    .unwrap_or(&"<UNKNOWN>".to_string()),
                input
                    .metadata
                    .artist
                    .as_ref()
                    .unwrap_or(&"<UNKNOWN>".to_string()),
            );

            state
                .http
                .create_message(msg.channel_id)
                .content(&content)?
                .exec()
                .await?;

            if let Some(call_lock) = state.songbird.get(guild_id) {
                let mut call = call_lock.lock().await;
                let handle = call.play_source(input);

                let mut store = state.trackdata.write().await;
                store.insert(guild_id, handle);
            }
        } else {
            state
                .http
                .create_message(msg.channel_id)
                .content("Didn't find any results")?
                .exec()
                .await?;
        }
    }
    Ok(())
}

async fn pause(
    msg: Message,
    state: State,
    _state_info: Arc<Mutex<StateInfo>>,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    let guild_id = msg.guild_id.unwrap();

    let store = state.trackdata.read().await;

    let content = if let Some(handle) = store.get(&guild_id) {
        let info = handle.get_info().await?;

        let paused = match info.playing {
            PlayMode::Play => {
                let _success = handle.pause();
                false
            }
            _ => {
                let _success = handle.play();
                true
            }
        };

        let action = if paused { "Unpaused" } else { "Paused" };

        format!("{} the track", action)
    } else {
        format!("No track to (un)pause!")
    };

    state
        .http
        .create_message(msg.channel_id)
        .content(&content)?
        .exec()
        .await?;

    Ok(())
}

async fn stop(
    msg: Message,
    state: State,
    _state_info: Arc<Mutex<StateInfo>>,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    let guild_id = msg.guild_id.unwrap();

    if let Some(call_lock) = state.songbird.get(guild_id) {
        let mut call = call_lock.lock().await;
        let _ = call.stop();
    }

    state
        .http
        .create_message(msg.channel_id)
        .content("Stopped the track")?
        .exec()
        .await?;

    Ok(())
}

async fn volume(
    msg: Message,
    state: State,
    _state_info: Arc<Mutex<StateInfo>>,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    state
        .http
        .create_message(msg.channel_id)
        .content("What's the volume you want to set (0.0-10.0, 1.0 being the default)?")?
        .exec()
        .await?;

    let author_id = msg.author.id;
    let msg = state
        .standby
        .wait_for_message(msg.channel_id, move |new_msg: &MessageCreate| {
            new_msg.author.id == author_id
        })
        .await?;
    let guild_id = msg.guild_id.unwrap();
    let volume = msg.content.parse::<f64>()?;

    if !volume.is_finite() || volume > 10.0 || volume < 0.0 {
        state
            .http
            .create_message(msg.channel_id)
            .content("Invalid volume!")?
            .exec()
            .await?;

        return Ok(());
    }

    let store = state.trackdata.read().await;

    let content = if let Some(handle) = store.get(&guild_id) {
        let _success = handle.set_volume(volume as f32);
        format!("Set the volume to {}", volume)
    } else {
        format!("No track to change volume!")
    };

    state
        .http
        .create_message(msg.channel_id)
        .content(&content)?
        .exec()
        .await?;

    Ok(())
}

async fn help(
    msg: Message,
    state: State,
    state_info: Arc<Mutex<StateInfo>>,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    if !state_info.lock().unwrap().is_joined {
        let res = join(msg.clone(), state.clone(), state_info.clone())
            .await
            .ok();

        match res {
            Some(result) => println!("{:?}", result),
            None => println!("ERR"),
        }
    }
    if state_info.lock().unwrap().is_joined {
        let file = File::open("help.txt").expect("err");
        let reader = BufReader::new(file);

        let mut embed_builder = EmbedBuilder::new();
        embed_builder = embed_builder.description("Commands:");

        for (index, line) in reader.lines().enumerate() {
            let data = line.unwrap();

            let f1 = EmbedFieldBuilder::new(String::from(index.to_string()), data)
                .inline()
                .build();
            embed_builder = embed_builder.field(f1);
        }

        let embed = embed_builder.validate()?.build();
        state
            .http
            .create_message(msg.channel_id)
            .embeds(&[embed])?
            .exec()
            .await?;
    }

    Ok(())
}

async fn time(
    msg: Message,
    state: State,
    state_info: Arc<Mutex<StateInfo>>,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    if !state_info.lock().unwrap().is_joined {
        let res = join(msg.clone(), state.clone(), state_info.clone())
            .await
            .ok();

        match res {
            Some(result) => println!("{:?}", result),
            None => println!("ERR"),
        }
    }
    if state_info.lock().unwrap().is_joined {
        let guild_id = msg.guild_id.unwrap();

        let store = state.trackdata.read().await;

        let content = if let Some(handle) = store.get(&guild_id) {
            let info = handle.get_info().await?;

            let time_elapsed = info.position;
            let time_elapsed_hours = (time_elapsed.as_secs() / 60) / 60;
            let time_elapsed_minutes = (time_elapsed.as_secs() / 60) % 60;
            let time_elapsed_seconds = time_elapsed.as_secs() % 60;
            let total_time = handle.metadata().duration;
            let total_time_hours = (total_time.unwrap_or(Duration::default()).as_secs() / 60) / 60;
            let total_time_minutes =
                (total_time.unwrap_or(Duration::default()).as_secs() / 60) % 60;
            let total_time_seconds = total_time.unwrap_or(Duration::default()).as_secs() % 60;

            format!(
                "{time_elapsed_hours:.1}H:{time_elapsed_minutes:.1}m:{time_elapsed_seconds:.1}s/{total_time_hours:.1}H:{total_time_minutes:.1}m:{total_time_seconds:.1}s"
            )
        } else {
            format!("Error gettting duration")
        };
        state
            .http
            .create_message(msg.channel_id)
            .content(&content)?
            .exec()
            .await?;
    }

    Ok(())
}

async fn description(
    msg: Message,
    state: State,
    state_info: Arc<Mutex<StateInfo>>,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    let song_link = state_info.lock().unwrap().current_song_link.clone();

    if !state_info.lock().unwrap().is_joined {
        let res = join(msg.clone(), state.clone(), state_info.clone())
            .await
            .ok();

        match res {
            Some(result) => println!("{:?}", result),
            None => println!("ERR"),
        }
    }
    if state_info.lock().unwrap().is_joined {
        let guild_id = msg.guild_id.unwrap();
        let store = state.trackdata.read().await;
        if let Some(handle) = store.get(&guild_id) {
            let h = handle.get_info().await;
            if h.is_ok() {
                let content = reqwest::get(song_link).await?.text().await?;

                let yt_struct = &yt_utils::get_link_content(content.as_str());

                state_info
                    .lock()
                    .unwrap()
                    .set_current_song_desc(yt_struct.get_yt_desc());

                let to_split = yt_struct.get_yt_desc();
                let re = Regex::new(r"\\n").unwrap();
                let result = re.replace_all(&to_split, " \n ");

                state
                    .http
                    .create_message(msg.channel_id)
                    .content(&result)?
                    .exec()
                    .await?;
            } else {
                state
                    .http
                    .create_message(msg.channel_id)
                    .content("`No song is currently playing!`")?
                    .exec()
                    .await?;
            }
        }
    }

    Ok(())
}

fn get_discord_token() -> String {
    let mut return_string: String = String::default();
    if Path::new("token.txt").exists() {
        return_string = read_to_string("token.txt").expect("Unable to open file");
    } else {
        let mut file1 = OpenOptions::new()
            .create(true)
            .write(true)
            .append(true)
            .open("token.txt")
            .unwrap();
        file1
            .write_all("<Insert discord token here>".as_bytes())
            .expect("err");
    }

    return_string
}