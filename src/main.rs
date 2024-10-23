use riven::consts::{Champion, GameType, Map, PlatformRoute};
use riven::RiotApi;

use std::sync::OnceLock;

use futures::{stream, StreamExt};

use std::collections::HashSet;

use chrono::{prelude::*, Local, TimeZone};

static CONFIG: OnceLock<Config> = OnceLock::new();

#[tokio::main]
async fn main() {
    let config = CONFIG.get_or_init(|| Config {
        platform_route: PlatformRoute::NA1,
        start_date: Local.with_ymd_and_hms(2024, 10, 1, 0, 0, 0).unwrap(),
        end_date: Local::now(),
    });

    let api_key = std::env::var("RGAPI_KEY").expect("RGAPI_KEY is not set in the environment");
    let game_name = std::env::var("NUZLOLCKE_GAME_NAME")
        .expect("NUZLOLCKE_GAME_NAME is not set in the environment");
    let tag_line = std::env::var("NUZLOLCKE_TAG_LINE")
        .expect("NUZLOLCKE_TAG_LINE is not set in the environment");
    let api = RiotApi::new(api_key);
    let puuid = get_summoner_puuid(&api, &game_name, &tag_line)
        .await
        .or_else(|| panic!("Can't find summoner {}#{}", game_name, tag_line))
        .unwrap();
    let loss_results =
        get_champion_losses_in_date_range(&api, &puuid, config.start_date, config.end_date).await;

    let champions_with_losses: Vec<Champion> = loss_results
        .iter()
        .map(|result| result.champion)
        .fold(
            (Vec::<Champion>::new(), HashSet::<Champion>::new()),
            |(mut champions, mut seen), champion| {
                if !seen.contains(&champion) {
                    seen.insert(champion);
                    champions.push(champion);
                }
                (champions, seen)
            },
        )
        .0;

    for champion in champions_with_losses {
        println!("{}", champion.name().unwrap_or("UNKNOWN"));
    }
}

async fn get_summoner_puuid(api: &RiotApi, game_name: &str, tag_line: &str) -> Option<String> {
    let platform_route = CONFIG
        .get()
        .expect("CONFIG is not set")
        .platform_route
        .to_regional();

    api.account_v1()
        .get_by_riot_id(platform_route, game_name, tag_line)
        .await
        .expect("Failed to get summoner")
        .map(|response| response.puuid)
}

async fn get_champion_losses_in_date_range(
    api: &RiotApi,
    puuid: &str,
    from: DateTime<Local>,
    to: DateTime<Local>,
) -> Vec<LossResult> {
    let platform_route = CONFIG
        .get()
        .expect("CONFIG is not set")
        .platform_route
        .to_regional();
    let mut match_ids: Vec<String> = Vec::new();
    let mut i = 0;
    loop {
        let matches = api
            .match_v5()
            .get_match_ids_by_puuid(
                platform_route,
                puuid,
                None,
                Some(to.timestamp()),
                None,
                Some(from.timestamp()),
                Some(i),
                None,
            )
            .await
            .expect("Failed to get matches");
        if matches.is_empty() {
            break;
        } else {
            i += matches.len() as i32;
            match_ids.extend(matches);
        }
    }
    stream::iter(match_ids)
        .filter_map(|match_id| async move {
            let match_data = api
                .match_v5()
                .get_match(platform_route, &match_id)
                .await
                .expect("Failed to get match data")?;
            let participant_idx = match_data
                .metadata
                .participants
                .iter()
                .position(|p| p == puuid)?;
            let participant = match_data.info.participants.get(participant_idx)?;
            let participant_team = match_data
                .info
                .teams
                .iter()
                .find(|team| team.team_id == participant.team_id)?;
            if !participant_team.win
                && matches!(match_data.info.game_type, Some(GameType::MATCHED_GAME))
                && matches!(
                    match_data.info.map_id,
                    Map::SUMMONERS_RIFT
                        | Map::SUMMONERS_RIFT_ORIGINAL_SUMMER_VARIANT
                        | Map::SUMMONERS_RIFT_ORIGINAL_AUTUMN_VARIANT
                )
            {
                Some(LossResult {
                    champion: participant.champion().ok()?,
                    date: Utc
                        .timestamp_millis_opt(match_data.info.game_start_timestamp)
                        .latest()?
                        .into(),
                    match_id: match_id.clone(),
                })
            } else {
                None
            }
        })
        .collect()
        .await
}

struct Config {
    platform_route: PlatformRoute,
    start_date: DateTime<Local>,
    end_date: DateTime<Local>,
}

struct LossResult {
    champion: Champion,
    date: DateTime<Local>,
    match_id: String,
}
