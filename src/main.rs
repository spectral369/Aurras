use async_trait::async_trait;
use futures::StreamExt;
use regex::Regex;
use songbird::{
    input::{ChildContainer, Compose, Input, YoutubeDl},
    tracks::{PlayMode, TrackHandle, TrackState},
    EventContext, EventHandler, Songbird,
};

use std::{
    collections::HashMap,
    error::Error,
    fs::{self, File, OpenOptions},
    future::Future,
    process::{self, Stdio},
    sync::Arc,
    time::Duration,
};
use std::{fs::read_to_string, io::prelude::*};
use tokio::time::{self};
use twilight_gateway::{
    cluster::{ClusterBuilder, ShardScheme},
    Event, Intents,
};
use twilight_model::{
    channel::Message,
    gateway::{
        payload::outgoing::update_presence::UpdatePresencePayload,
        presence::{Activity, ActivityType, Status},
    },
    id::{marker::GuildMarker, Id},
};

use std::io::{BufRead, BufReader};
use tokio::sync::{Mutex, RwLock};

use std::process::Command;
use twilight_cache_inmemory::{InMemoryCache, ResourceType};

use twilight_http::Client as HttpClient;

use std::time::Instant;
use twilight_standby::Standby;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder, ImageSource};

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

struct Queue1 {
    queue: Vec<YoutubeDl>,
}

#[async_trait]
impl EventHandler for Queue1 {
    async fn act(&self, _ctx: &EventContext<'_>) -> Option<songbird::Event> {
        if !self.queue.is_empty() {
            let mut _src = self.queue[0].clone();
        }

        println!("song finished ");
        return None;
    }
}

impl Queue1 {
    pub fn remove_fist(&mut self) {
        if self.queue.len() > 0 {
            self.queue.remove(0);
        }
    }
}

struct StateInfo {
    is_joined: bool,
    current_song_desc: String,
    current_song_link: String,
    _yt_utils: yt_utils::YtInfo,
    current_song_length: Option<Duration>,
    is_playing: bool,
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
    pub fn set_current_song_length(&mut self, value: Option<Duration>) {
        self.current_song_length = value;
    }
    pub fn set_is_playing(&mut self, value: bool) {
        self.is_playing = value;
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
    let (mut events, state, state_info, queue) = {
        let token = get_discord_token();
        if token.len() < 30 {
            println!("{:?} - {}", token, "Is not valid token !");
            process::exit(0x0100);
        }
        let http = HttpClient::new(token.clone());
        let user_id = http.current_user().await?.model().await?.id;

        let intents = Intents::GUILD_MESSAGES
            | Intents::DIRECT_MESSAGES
            | Intents::GUILD_MEMBERS
            | Intents::GUILDS
            | Intents::GUILD_VOICE_STATES
            | Intents::MESSAGE_CONTENT;

        let cluster_id = 0;
        let clusters = 1;
        let shards_per_cluster = 1;

        let (cluster, events) = ClusterBuilder::new(token, intents)
            .shard_scheme(ShardScheme::try_from((
                (cluster_id * shards_per_cluster..(cluster_id + 1) * shards_per_cluster),
                shards_per_cluster * clusters,
            ))?)
            .presence(UpdatePresencePayload {
                activities: vec![Activity {
                    application_id: None,
                    assets: None,
                    buttons: vec![],
                    created_at: None,
                    details: None,
                    emoji: None,
                    flags: None,
                    id: None,
                    instance: None,
                    kind: ActivityType::Playing,
                    name: "RUST BOT DEMO".to_string(),
                    party: None,
                    secrets: None,
                    state: None,
                    timestamps: None,
                    url: None,
                }],
                afk: false,
                since: None,
                status: Status::Online,
            })
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
                    .resource_types(ResourceType::VOICE_STATE | ResourceType::GUILD)
                    .build(),
            }),
            Arc::new(Mutex::new(StateInfo {
                is_joined: false,
                current_song_desc: String::default(),
                current_song_link: String::default(),
                _yt_utils: Default::default(),
                current_song_length: Option::default(),
                is_playing: false,
            })),
            Arc::new(Mutex::new(Queue1 {
                queue: Vec::default(),
            })),
        )
    };

    while let Some((_, event)) = events.next().await {
        //id
        // let ev = event.clone();
        state.standby.process(&event);
        state.cache.update(&event);
        state.songbird.process(&event).await;

        if let Event::MessageCreate(msg) = event {
            let msg2 = msg.clone();
            if msg.guild_id.is_none() || !msg.content.starts_with('!') {
                continue;
            }

            match msg.content.splitn(2, ' ').next() {
                Some("!join") => spawn(join(msg.0, Arc::clone(&state), Arc::clone(&state_info))),
                Some("!leave") => spawn(leave(msg.0, Arc::clone(&state), Arc::clone(&state_info))),
                Some("!pause") => spawn(pause(msg.0, Arc::clone(&state), Arc::clone(&state_info))),
                Some("!play") => spawn(play(
                    msg.0,
                    Arc::clone(&state),
                    Arc::clone(&state_info),
                    Arc::clone(&queue),
                )),
                Some("!help") => spawn(help(msg.0, Arc::clone(&state), Arc::clone(&state_info))),
                Some("!radiolist") => spawn(radiolist(
                    msg.0,
                    Arc::clone(&state),
                    Arc::clone(&state_info),
                )),
                Some("!stop") => spawn(stop(msg.0, Arc::clone(&state), Arc::clone(&state_info))),
                Some("!time") => spawn(time(msg.0, Arc::clone(&state), Arc::clone(&state_info))),
                Some("!add") => spawn(add(
                    msg.0,
                    Arc::clone(&state),
                    Arc::clone(&state_info),
                    Arc::clone(&queue),
                )),
                Some("!list") => spawn(list(
                    msg.0,
                    Arc::clone(&state),
                    Arc::clone(&state_info),
                    Arc::clone(&queue),
                )),
                Some("!desc") => spawn(description(
                    msg.0,
                    Arc::clone(&state),
                    Arc::clone(&state_info),
                )),
                Some("!radiozu") => {
                    spawn(radiozu(msg.0, Arc::clone(&state), Arc::clone(&state_info)))
                }
                Some("!radiovirgin") => spawn(radiovirgin(
                    msg.0,
                    Arc::clone(&state),
                    Arc::clone(&state_info),
                )),
                Some("!radio24house") => spawn(radio24house(
                    msg.0,
                    Arc::clone(&state),
                    Arc::clone(&state_info),
                )),
                Some("!radioclubbers") => spawn(radioclubbers(
                    msg.0,
                    Arc::clone(&state),
                    Arc::clone(&state_info),
                )),
                Some("!radiouv") => spawn(radiouv(
                    msg.0,
                    Arc::clone(&state),
                    Arc::clone(&state_info),
                )),
                Some("!radiohouse") => spawn(radiohouse(
                    msg.0,
                    Arc::clone(&state),
                    Arc::clone(&state_info),
                )),
                Some("!radiodancefm") => spawn(radiodancefmro(
                    msg.0,
                    Arc::clone(&state),
                    Arc::clone(&state_info),
                )),
                Some("!volume") => {
                    spawn(volume(msg.0, Arc::clone(&state), Arc::clone(&state_info)))
                }
                Some("!repeat") => spawn(time(msg.0, Arc::clone(&state), Arc::clone(&state_info))),
                _ => continue,
            }
            if state_info.lock().await.is_joined {
                let guild_id = msg2.guild_id.ok_or("No guild id")?;

                let user_id = msg2.author.id;

                let voice_id = state
                    .cache
                    .voice_state(user_id, guild_id.clone())
                    .unwrap()
                    .channel_id();

                let u = state.clone();
                let mut is_finished: bool = false;
                let mut interval = time::interval(Duration::from_secs(25));
                tokio::spawn(async move {
                    let h = Arc::clone(&u);

                    'mymainloop: loop {
                        let j = Arc::clone(&h);

                        tokio::spawn(async move {
                            let nr_mem = j.cache.voice_channel_states(voice_id).unwrap().count();
                            let bot_id: u64 = 920990750030848030;
                            let mut mem_ids =
                                j.cache.voice_channel_states(voice_id).unwrap().enumerate();

                            let mut ids: Vec<u64> = Vec::new();

                            let mut i = 0;
                            'test: loop {
                                let id1 = mem_ids.next().unwrap().1.user_id();
                                //   println!("ID: {}", id1);
                                ids.push(id1.get());
                                i = i + 1;
                                if i == nr_mem {
                                    break 'test;
                                }
                            }

                            //println!("no joined: {}", nr_mem);
                            if nr_mem < 2 && ids.contains(&bot_id) {
                                //   j.songbird.leave(guild_id).await;
                                let _res = j.songbird.remove(guild_id).await;
                                is_finished = true;
                            }
                        });
                        if is_finished {
                            break 'mymainloop;
                        }
                        interval.tick().await;
                    }
                });
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
                .await?;
            return Ok(());
        }
    }
    let (_handle, success) = state
        .songbird
        .join(
            guild_id.into_nonzero(),
            state
                .cache
                .voice_state(user_id, guild_id.clone())
                .unwrap()
                .value()
                .channel_id(),
        )
        .await;

    let content: String = match success {
        Ok(()) => {
            state_info.lock().await.set_is_joined(true);
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
        .await?;

    Ok(())
}

async fn leave(
    msg: Message,
    state: State,
    state_info: Arc<Mutex<StateInfo>>,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    let guild_id = msg.guild_id.unwrap();
    state_info.lock().await.set_is_joined(false);
    state.songbird.leave(guild_id).await?;

    state
        .http
        .create_message(msg.channel_id)
        .content("Left the channel")?
        .await?;

    Ok(())
}

async fn play(
    msg: Message,
    state: State,
    state_info: Arc<Mutex<StateInfo>>,
    queue: Arc<Mutex<Queue1>>,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    let now = Instant::now();
    if !state_info.lock().await.is_joined {
        let res = join(msg.clone(), state.clone(), state_info.clone())
            .await
            .ok();

        match res {
            Some(result) => println!("{:?}", result),
            None => println!("ERR"),
        }
    }
    if state_info.lock().await.is_joined {
        let b = msg.content.clone();
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
                &yt_utils::_extract_links(content.as_str())
                    .iter()
                    //  .skip(1)
                    .next()
                    .unwrap()
                    .to_string(),
            );
            state_info
                .lock()
                .await
                .set_current_song_link(yt_link.clone());
        }
        //check if it's playing

        let guild_id = msg.guild_id.unwrap();

        let mut que1 = queue.lock().await;
        let queue_list = que1.queue.clone();
        que1.remove_fist();

        //   let queue_list = &queue.lock().await.queue;
        let mut src;
        if !queue_list.is_empty() {
            src = queue_list[0].clone();
        } else {
            src = YoutubeDl::new(reqwest::Client::new(), yt_link);
        }

        //    let mut src = YoutubeDl::new(reqwest::Client::new(), yt_link);
        if let Ok(metadata) = src.aux_metadata().await {
            let content = format!(
                "Playing **{:?}**",
                metadata.title.as_ref().unwrap_or(&"<UNKNOWN>".to_string()),
            );
            state_info
                .lock()
                .await
                .set_current_song_length(metadata.duration);

            state
                .http
                .create_message(msg.channel_id)
                .content(&content)?
                .await?;

            if let Some(call_lock) = state.songbird.get(guild_id) {
                if state_info.lock().await.is_playing {
                    let mut call = call_lock.lock().await;
                    let _ = call.stop();
                    state_info.lock().await.set_is_playing(false);
                }

                let mut call = call_lock.lock().await;
                let handle = call.play_input(src.into());
                state_info.lock().await.set_is_playing(true);

                let mut store = state.trackdata.write().await;
                store.insert(guild_id, handle);

                let gr = store.get_key_value(&guild_id);

                let y = gr.unwrap().1;

                let queue2 = &queue_list.clone();

                let queue = queue2.clone();

                let _res = y.add_event(
                    songbird::Event::Track(songbird::TrackEvent::End),
                    Queue1 { queue },
                );
            }
        } else {
            state
                .http
                .create_message(msg.channel_id)
                .content("Didn't find any results")?
                .await?;
        }
    }
    let elapsed = now.elapsed();
    println!("Elapsed Youtube: {:.2?}", elapsed);
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
        .await?;

    Ok(())
}

async fn stop(
    msg: Message,
    state: State,
    state_info: Arc<Mutex<StateInfo>>,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    let guild_id = msg.guild_id.unwrap();

    if let Some(call_lock) = state.songbird.get(guild_id.into_nonzero()) {
        let mut call = call_lock.lock().await;
        let _ = call.stop();
        state_info.lock().await.set_is_playing(false);
    }

    state
        .http
        .create_message(msg.channel_id)
        .content("Stopped the track")?
        .await?;

    Ok(())
}

async fn volume(
    msg: Message,
    state: State,
    _state_info: Arc<Mutex<StateInfo>>,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    let guild_id = msg.guild_id.unwrap();
    let mut content = msg.content.clone();
    content = content[7..].to_string().to_string();
    content.retain(|f| !f.is_whitespace());

    if content.to_string().is_empty() {
        state
            .http
            .create_message(msg.channel_id)
            .content("Use !volume <value>")?
            .await?;
    } else {
        let volume = content.parse::<f32>()?;

        if !volume.is_finite() || volume > 10.0 || volume < 0.0 {
            state
                .http
                .create_message(msg.channel_id)
                .content("Invalid volume!")?
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
            .await?;
    }

    Ok(())
}

async fn help(
    msg: Message,
    state: State,
    state_info: Arc<Mutex<StateInfo>>,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    if !state_info.lock().await.is_joined {
        let res = join(msg.clone(), state.clone(), state_info.clone())
            .await
            .ok();

        match res {
            Some(result) => println!("{:?}", result),
            None => println!("ERR"),
        }
    }
    if state_info.lock().await.is_joined {
        let path = fs::canonicalize("./help.txt");
        let file = File::open(path.unwrap()).expect("err");
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
            .await?;
    }

    Ok(())
}

async fn radiolist(
    msg: Message,
    state: State,
    state_info: Arc<Mutex<StateInfo>>,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    if !state_info.lock().await.is_joined {
        let res = join(msg.clone(), state.clone(), state_info.clone())
            .await
            .ok();

        match res {
            Some(result) => println!("{:?}", result),
            None => println!("ERR"),
        }
    }
    if state_info.lock().await.is_joined {
        let path = fs::canonicalize("./radiolist.txt");
        let file = File::open(path.unwrap()).expect("err");
        let reader = BufReader::new(file);

        let mut embed_builder = EmbedBuilder::new();
        embed_builder = embed_builder.description("Radio List:");

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
            .await?;
    }

    Ok(())
}

async fn time(
    msg: Message,
    state: State,
    state_info: Arc<Mutex<StateInfo>>,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    if !state_info.lock().await.is_joined {
        let res = join(msg.clone(), state.clone(), state_info.clone())
            .await
            .ok();

        match res {
            Some(result) => println!("{:?}", result),
            None => println!("ERR"),
        }
    }
    if state_info.lock().await.is_joined {
        let guild_id = msg.guild_id.unwrap();

        let store = state.trackdata.read().await;

        let content = if let Some(handle) = store.get(&guild_id) {
            let info: TrackState = handle.get_info().await?;

            let time_elapsed = info.position;
            let time_elapsed_hours = (time_elapsed.as_secs() / 60) / 60;
            let time_elapsed_minutes = (time_elapsed.as_secs() / 60) % 60;
            let time_elapsed_seconds = time_elapsed.as_secs() % 60;
            let total_time = state_info
                .lock()
                .await
                .current_song_length
                .unwrap_or(Duration::default());
            let total_time_hours = (total_time.as_secs() / 60) / 60;
            let total_time_minutes = (total_time.as_secs() / 60) % 60;
            let total_time_seconds = total_time.as_secs() % 60;

            format!(
                "{time_elapsed_hours:.1}H:{time_elapsed_minutes:.1}m:{time_elapsed_seconds:.1}s/{total_time_hours:.1}H:{total_time_minutes:.1}m:{total_time_seconds:.1}s"
            )
        } else {
            format!("Error gettting duration")
        };
        let mut part1: String = "`".to_string().to_owned();
        part1.push_str(&content);
        part1.push_str("`");

        state
            .http
            .create_message(msg.channel_id)
            .content(&part1)?
            .await?;
    }

    Ok(())
}

async fn list(
    msg: Message,
    state: State,
    state_info: Arc<Mutex<StateInfo>>,
    queue: Arc<Mutex<Queue1>>,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    if !state_info.lock().await.is_joined {
        let res = join(msg.clone(), state.clone(), state_info.clone())
            .await
            .ok();

        match res {
            Some(result) => println!("{:?}", result),
            None => println!("ERR"),
        }
    }
    let mut counter: i16 = 0;
    if state_info.lock().await.is_joined {
        let list = &queue.lock().await.queue;

        if list.is_empty() {
            state
                .http
                .create_message(msg.channel_id)
                .content(&"No songs in queue!")?
                .await?;
        } else {
            for item in list {
                counter = counter + 1;
                let dat = item.clone().aux_metadata().await;
                let title = dat
                    .unwrap_or_default()
                    .title
                    .unwrap_or("UNKNOWN".to_string());

                let mut content = String::from("*");
                content.push_str(&counter.to_string());
                content.push_str("* - ");
                content.push_str(&title);

                state
                    .http
                    .create_message(msg.channel_id)
                    .content(&content)?
                    .await?;
            }
        }
    }

    Ok(())
}

async fn add(
    msg: Message,
    state: State,
    state_info: Arc<Mutex<StateInfo>>,
    queue: Arc<Mutex<Queue1>>,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    if !state_info.lock().await.is_joined {
        let res = join(msg.clone(), state.clone(), state_info.clone())
            .await
            .ok();

        match res {
            Some(result) => println!("{:?}", result),
            None => println!("ERR"),
        }
    }
    if state_info.lock().await.is_joined {
        let guild_id = msg.guild_id.unwrap();

        let b = msg.content.clone();
        let index1 = b.find(" ");
        let mut text: String;
        if !index1.is_none() {
            text = b.chars().skip(index1.unwrap()).collect();
        } else {
            text = "".to_string();
        }
        text = text.trim().replace(" ", "+");

        if let Some(_call_lock) = state.songbird.get(guild_id.into_nonzero()) {
            // let mut call = call_lock.lock().await;
            // let queue = queue.queues.entry(songbird::id::GuildId(guild_id.get()))

            // let queue_q = queue.lock().unwrap().queues.entry(songbird::id::GuildId(guild_id.get())).or_default();

            println!("{:?}", text);
            /*let source = ytdl(text)
            .await
              .expect("This might fail: handle this error!");*/

            let mut source = YoutubeDl::new(reqwest::Client::new(), text);

            //   let title =  source.metadata.title.as_ref().unwrap().clone();
            let mut title = "".to_string();
            if let Ok(metadata) = source.aux_metadata().await {
                let content = format!(
                    "**{:?}** added !",
                    metadata.title.as_ref().unwrap_or(&"<UNKNOWN>".to_string()),
                );
                title = content.clone();
            }

            // Queueing a track is this easy!
            //let hnd = queue.add_source(source.into(), &mut call);
            queue.lock().await.queue.push(source);

            state
                .http
                .create_message(msg.channel_id)
                .content(&title)?
                .await?;
        }
    }

    Ok(())
}

async fn description(
    msg: Message,
    state: State,
    state_info: Arc<Mutex<StateInfo>>,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    let song_link = state_info.lock().await.current_song_link.clone();

    if !state_info.lock().await.is_joined {
        let res = join(msg.clone(), state.clone(), state_info.clone())
            .await
            .ok();

        match res {
            Some(result) => println!("{:?}", result),
            None => println!("ERR"),
        }
    }
    if state_info.lock().await.is_joined {
        let guild_id = msg.guild_id.unwrap();
        let store = state.trackdata.read().await;
        if let Some(handle) = store.get(&guild_id) {
            let h = handle.get_info().await;
            if h.is_ok() {
                let content = reqwest::get(&song_link).await?.text().await?;

                let yt_struct = &yt_utils::get_link_content(content.as_str(), song_link.clone());

                state_info
                    .lock()
                    .await
                    .set_current_song_desc(yt_struct.get_yt_desc());

                let to_split = yt_struct.get_yt_desc();
                let re = Regex::new(r"\\n").unwrap();
                let result = re.replace_all(&to_split, "\n");
                let re2 = Regex::new(r"(https://)|(http://)").unwrap();
                let result2 = re2.replace_all(&result, "[http][//]");

                let re3 = Regex::new(r"\n\n").unwrap();
                let result3 = re3.replace_all(&result2, "\n");
                let result_final: String = result3.chars().take(1999).collect();
                state
                    .http
                    .create_message(msg.channel_id)
                    .content(&result_final)?
                    .await?;
            } else {
                state
                    .http
                    .create_message(msg.channel_id)
                    .content("`No song is currently playing!`")?
                    .await?;
            }
        }
    }

    Ok(())
}

async fn radiozu(
    msg: Message,
    state: State,
    state_info: Arc<Mutex<StateInfo>>,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    if !state_info.lock().await.is_joined {
        let res = join(msg.clone(), state.clone(), state_info.clone())
            .await
            .ok();

        match res {
            Some(result) => println!("{:?}", result),
            None => println!("ERR"),
        }
    }
    if state_info.lock().await.is_joined {
        let a = Command::new("ffmpeg")
            .arg("-i")
            .arg("https://live4ro.antenaplay.ro/radiozu/radiozu-48000.m3u8")
            .arg("-f")
            .arg("wav")
            .arg("-ac")
            .arg("2")
            .arg("-acodec")
            .arg("pcm_s16le")
            .arg("-ar")
            .arg("48000")
            .arg("-")
            .stdout(Stdio::piped())
            .spawn();
        let test: Input = ChildContainer::from(a.unwrap()).into();
        let guild_id = msg.guild_id.unwrap();
        let source = ImageSource::url(
            "https://static.tuneyou.com/images/logos/500_500/33/3133/RadioZU.jpg",
        )?;

        let embed = EmbedBuilder::new()
            .title("RadioZU Romania")
            .field(EmbedFieldBuilder::new("Requestor", msg.author.name).inline())
            .image(source)
            .validate()?
            .build();

        state
            .http
            .create_message(msg.channel_id)
            .embeds(&[embed])?
            .await?;
        if let Some(call_lock) = state.songbird.get(guild_id) {
            if state_info.lock().await.is_playing {
                let mut call = call_lock.lock().await;
                let _ = call.stop();
                state_info.lock().await.set_is_playing(false);
            }

            let mut call = call_lock.lock().await;
            let handle = call.play_input(test);
            state_info.lock().await.set_is_playing(true);

            let mut store = state.trackdata.write().await;
            store.insert(guild_id, handle);
        }
    }

    Ok(())
}

async fn radio24house(
    msg: Message,
    state: State,
    state_info: Arc<Mutex<StateInfo>>,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    if !state_info.lock().await.is_joined {
        let res = join(msg.clone(), state.clone(), state_info.clone())
            .await
            .ok();

        match res {
            Some(result) => println!("{:?}", result),
            None => println!("ERR"),
        }
    }
    if state_info.lock().await.is_joined {
        let a = Command::new("ffmpeg")
            .arg("-i")
            .arg("https://24houseradio-adradio.radioca.st/128")
            .arg("-f")
            .arg("wav")
            .arg("-ac")
            .arg("2")
            .arg("-acodec")
            .arg("pcm_s16le")
            .arg("-ar")
            .arg("48000")
            .arg("-")
            .stdout(Stdio::piped())
            .spawn();
        let test: Input = ChildContainer::from(a.unwrap()).into();
        let guild_id = msg.guild_id.unwrap();
        let source = ImageSource::url(
            "https://cdn2.vectorstock.com/i/1000x1000/01/16/radio-music-neon-logo-night-neon-vector-21420116.jpg",
        )?;

        let embed = EmbedBuilder::new()
            .title("Radio 24 House")
            .field(EmbedFieldBuilder::new("Requestor", msg.author.name).inline())
            .image(source)
            .validate()?
            .build();

        state
            .http
            .create_message(msg.channel_id)
            .embeds(&[embed])?
            .await?;
        if let Some(call_lock) = state.songbird.get(guild_id) {
            if state_info.lock().await.is_playing {
                let mut call = call_lock.lock().await;
                let _ = call.stop();
                state_info.lock().await.set_is_playing(false);
            }

            let mut call = call_lock.lock().await;
            let handle = call.play_input(test);
            state_info.lock().await.set_is_playing(true);

            let mut store = state.trackdata.write().await;
            store.insert(guild_id, handle);
        }
    }

    Ok(())
}

async fn radioclubbers(
    msg: Message,
    state: State,
    state_info: Arc<Mutex<StateInfo>>,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    if !state_info.lock().await.is_joined {
        let res = join(msg.clone(), state.clone(), state_info.clone())
            .await
            .ok();

        match res {
            Some(result) => println!("{:?}", result),
            None => println!("ERR"),
        }
    }
    if state_info.lock().await.is_joined {
        let a = Command::new("ffmpeg")
            .arg("-i")
            .arg("https://s3.slotex.pl/shoutcast/7300/stream?sid=1")
            .arg("-f")
            .arg("wav")
            .arg("-ac")
            .arg("2")
            .arg("-acodec")
            .arg("pcm_s16le")
            .arg("-ar")
            .arg("48000")
            .arg("-")
            .stdout(Stdio::piped())
            .spawn();
        let test: Input = ChildContainer::from(a.unwrap()).into();
        let guild_id = msg.guild_id.unwrap();
        let source = ImageSource::url(
            "https://cdn2.vectorstock.com/i/1000x1000/01/16/radio-music-neon-logo-night-neon-vector-21420116.jpg",
        )?;

        let embed = EmbedBuilder::new()
            .title("Radio Clubbers")
            .field(EmbedFieldBuilder::new("Requestor", msg.author.name).inline())
            .image(source)
            .validate()?
            .build();

        state
            .http
            .create_message(msg.channel_id)
            .embeds(&[embed])?
            .await?;
        if let Some(call_lock) = state.songbird.get(guild_id) {
            if state_info.lock().await.is_playing {
                let mut call = call_lock.lock().await;
                let _ = call.stop();
                state_info.lock().await.set_is_playing(false);
            }

            let mut call = call_lock.lock().await;
            let handle = call.play_input(test);
            state_info.lock().await.set_is_playing(true);

            let mut store = state.trackdata.write().await;
            store.insert(guild_id, handle);
        }
    }

    Ok(())
}

async fn radiouv(
    msg: Message,
    state: State,
    state_info: Arc<Mutex<StateInfo>>,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    if !state_info.lock().await.is_joined {
        let res = join(msg.clone(), state.clone(), state_info.clone())
            .await
            .ok();

        match res {
            Some(result) => println!("{:?}", result),
            None => println!("ERR"),
        }
    }
    if state_info.lock().await.is_joined {
        let a = Command::new("ffmpeg")
            .arg("-i")
            .arg("https://stream-21.zeno.fm/s98kga59qnruv?zs=y9vZRej5RV69g4Ld8fD7QQ")
            .arg("-f")
            .arg("wav")
            .arg("-ac")
            .arg("2")
            .arg("-acodec")
            .arg("pcm_s16le")
            .arg("-ar")
            .arg("48000")
            .arg("-")
            .stdout(Stdio::piped())
            .spawn();
        let test: Input = ChildContainer::from(a.unwrap()).into();
        let guild_id = msg.guild_id.unwrap();
        let source = ImageSource::url(
            "https://cdn2.vectorstock.com/i/1000x1000/01/16/radio-music-neon-logo-night-neon-vector-21420116.jpg",
        )?;

        let embed = EmbedBuilder::new()
            .title("Radio Underground Vibe")
            .field(EmbedFieldBuilder::new("Requestor", msg.author.name).inline())
            .image(source)
            .validate()?
            .build();

        state
            .http
            .create_message(msg.channel_id)
            .embeds(&[embed])?
            .await?;
        if let Some(call_lock) = state.songbird.get(guild_id) {
            if state_info.lock().await.is_playing {
                let mut call = call_lock.lock().await;
                let _ = call.stop();
                state_info.lock().await.set_is_playing(false);
            }

            let mut call = call_lock.lock().await;
            let handle = call.play_input(test);
            state_info.lock().await.set_is_playing(true);

            let mut store = state.trackdata.write().await;
            store.insert(guild_id, handle);
        }
    }

    Ok(())
}

async fn radiodancefmro(
    msg: Message,
    state: State,
    state_info: Arc<Mutex<StateInfo>>,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    if !state_info.lock().await.is_joined {
        let res = join(msg.clone(), state.clone(), state_info.clone())
            .await
            .ok();

        match res {
            Some(result) => println!("{:?}", result),
            None => println!("ERR"),
        }
    }
    if state_info.lock().await.is_joined {
        let a = Command::new("ffmpeg")
            .arg("-i")
            .arg("https://edge126.rcs-rds.ro/profm/dancefm.mp3")
            .arg("-f")
            .arg("wav")
            .arg("-ac")
            .arg("2")
            .arg("-acodec")
            .arg("pcm_s16le")
            .arg("-ar")
            .arg("48000")
            .arg("-")
            .stdout(Stdio::piped())
            .spawn();
        let test: Input = ChildContainer::from(a.unwrap()).into();
        let guild_id = msg.guild_id.unwrap();
        let source = ImageSource::url(
            "https://cdn2.vectorstock.com/i/1000x1000/01/16/radio-music-neon-logo-night-neon-vector-21420116.jpg",
        )?;

        let embed = EmbedBuilder::new()
            .title("DanceFM.RO")
            .field(EmbedFieldBuilder::new("Requestor", msg.author.name).inline())
            .image(source)
            .validate()?
            .build();

        state
            .http
            .create_message(msg.channel_id)
            .embeds(&[embed])?
            .await?;
        if let Some(call_lock) = state.songbird.get(guild_id) {
            if state_info.lock().await.is_playing {
                let mut call = call_lock.lock().await;
                let _ = call.stop();
                state_info.lock().await.set_is_playing(false);
            }

            let mut call = call_lock.lock().await;
            let handle = call.play_input(test);
            state_info.lock().await.set_is_playing(true);

            let mut store = state.trackdata.write().await;
            store.insert(guild_id, handle);
        }
    }

    Ok(())
}


async fn radiohouse(
    msg: Message,
    state: State,
    state_info: Arc<Mutex<StateInfo>>,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    if !state_info.lock().await.is_joined {
        let res = join(msg.clone(), state.clone(), state_info.clone())
            .await
            .ok();

        match res {
            Some(result) => println!("{:?}", result),
            None => println!("ERR"),
        }
    }
    if state_info.lock().await.is_joined {
        let a = Command::new("ffmpeg")
            .arg("-i")
            .arg("https://deephouseradio.radioca.st/deep?type=http&nocache=16")
            .arg("-f")
            .arg("wav")
            .arg("-ac")
            .arg("2")
            .arg("-acodec")
            .arg("pcm_s16le")
            .arg("-ar")
            .arg("48000")
            .arg("-")
            .stdout(Stdio::piped())
            .spawn();
        let test: Input = ChildContainer::from(a.unwrap()).into();
        let guild_id = msg.guild_id.unwrap();
        let source = ImageSource::url(
            "https://cdn2.vectorstock.com/i/1000x1000/01/16/radio-music-neon-logo-night-neon-vector-21420116.jpg",
        )?;

        let embed = EmbedBuilder::new()
            .title("Radio House Santa Monica")
            .field(EmbedFieldBuilder::new("Requestor", msg.author.name).inline())
            .image(source)
            .validate()?
            .build();

        state
            .http
            .create_message(msg.channel_id)
            .embeds(&[embed])?
            .exec()
            .await?;
        if let Some(call_lock) = state.songbird.get(guild_id) {
            if state_info.lock().await.is_playing {
                let mut call = call_lock.lock().await;
                let _ = call.stop();
                state_info.lock().await.set_is_playing(false);
            }

            let mut call = call_lock.lock().await;
            let handle = call.play_input(test);
            state_info.lock().await.set_is_playing(true);

            let mut store = state.trackdata.write().await;
            store.insert(guild_id, handle);
        }
    }

    Ok(())
}

async fn radiovirgin(
    msg: Message,
    state: State,
    state_info: Arc<Mutex<StateInfo>>,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    let now = Instant::now();
    if !state_info.lock().await.is_joined {
        let res = join(msg.clone(), state.clone(), state_info.clone())
            .await
            .ok();

        match res {
            Some(result) => println!("{:?}", result),
            None => println!("ERR"),
        }
    }
    if state_info.lock().await.is_joined {
        let a = Command::new("ffmpeg")
            .arg("-i")
            .arg("https://astreaming.edi.ro:8443/VirginRadio_aac")
            .arg("-f")
            .arg("wav")
            .arg("-ac")
            .arg("2")
            .arg("-acodec")
            .arg("pcm_s16le")
            .arg("-ar")
            .arg("48000")
            .arg("-")
            .stdout(Stdio::piped())
            .spawn();
        let test: Input = ChildContainer::from(a.unwrap()).into();
        let guild_id = msg.guild_id.unwrap();

        let source = ImageSource::url(
            "https://virginradio.ro/wp-content/uploads/2019/06/VR_ROMANIA_WHITE-STAR-LOGO_RGB_ONLINE_1600x1600.png",
        )?;
        let embed = EmbedBuilder::new()
            .title("Virgin Radio Romania")
            .field(EmbedFieldBuilder::new("Requestor", msg.author.name).inline())
            .image(source)
            .validate()?
            .build();

        state
            .http
            .create_message(msg.channel_id)
            .embeds(&[embed])?
            .await?;

        if let Some(call_lock) = state.songbird.get(guild_id) {
            if state_info.lock().await.is_playing {
                let mut call = call_lock.lock().await;
                let _ = call.stop();
                state_info.lock().await.set_is_playing(false);
            }

            let mut call = call_lock.lock().await;
            let handle = call.play_input(test);
            state_info.lock().await.set_is_playing(true);

            /* let activity = Activity::from(MinimalActivity {
                kind: ActivityType::Custom,
                name: "Playing Virgin Radio Romania".to_owned(),
                url: None,
            });
            let request = UpdatePresence::new(Vec::from([activity]), false, None, Status::Online)?;

            // Send the request over the shard.
            // let _rest = state.cluster.command(2, &request).await;
            //  let shard_id =  state.cluster.shards().next().take().unwrap().info().unwrap().id();
            let shard_id = state
                .cluster
                .shards()
                .enumerate()
                .next()
                .unwrap()
                .1
                .info()
                .unwrap()
                .id();
            println!("{:?}", shard_id);
            let _rest = state.cluster.command(1, &request).await;
            // println!("Next line is the shit");
            println!("{:?}", _rest);*/

            let mut store = state.trackdata.write().await;
            store.insert(guild_id, handle);
        }

        let elapsed = now.elapsed();
        println!("Elapsed Radio: {:.2?}", elapsed);
    }

    Ok(())
}

fn get_discord_token() -> String {
    let mut return_string: String = String::default();
    let path = fs::canonicalize("./token.txt").unwrap();
    if path.exists() {
        return_string = read_to_string(path).expect("Unable to open file");
    } else {
        let mut file1 = OpenOptions::new()
            .create(true)
            .write(true)
            .append(true)
            .read(true)
            .open(fs::canonicalize("./token.txt").unwrap())
            .unwrap();
        file1
            .write_all("<Insert discord token here>".as_bytes())
            .expect("err");
    }

    return_string
}
