pub mod comments;
mod length;
mod likes;
pub mod models;
pub mod panels;

use crate::factories::BlockingReferClientFactory;
use crate::story::chapter::comments::models::UserComment;
use crate::story::chapter::models::Chapter;
use crate::{Arc, Season, SeasonChapter, Skip};
use anyhow::{bail, Context, Result};
use indicatif::ParallelProgressIterator;
use rayon::prelude::*;
use reqwest::StatusCode;
use scraper::{Html, Selector};
use tracing::error;

/// # Errors
pub fn parse(
    start: u16,
    end: u16,
    id: u32,
    season: Season,
    season_chapter: SeasonChapter,
    arc: Arc,
    skip: Skip,
) -> Result<Vec<Chapter>> {
    // 8 Threads is around the line at which problems start to occur when pinging out too many times at once as all getting blocked
    rayon::ThreadPoolBuilder::new()
        .num_threads(8)
        .build_global()
        .context("Couldn't create thread pool")?;

    let chapters: Vec<_> = (start..=end).collect();
    let total = chapters.len() as u64;

    // TODO: Figure out proper error propagation
    let vec = chapters
        .into_par_iter()
        .progress_count(total)
        .filter_map(|chap| chapter(id, chap, season, season_chapter, arc, skip))
        .collect();

    Ok(vec)
}

fn chapter(
    id: u32,
    chapter: u16,
    season_fn: Season,
    season_chapter_fn: SeasonChapter,
    arc_fn: Arc,
    skip: Skip,
) -> Option<Chapter> {
    let url = chapter_url(id, chapter);

    let response = BlockingReferClientFactory::get(&url).unwrap();

    if response.status() != StatusCode::OK || skip(chapter) {
        return None;
    }

    let text = response.text().unwrap();

    let html = Html::parse_document(&text);

    let likes =
        likes::parse(id, chapter).unwrap_or_else(|_| panic!("failed to parse likes from {url}"));

    let length =
        length::parse(&html).unwrap_or_else(|_| panic!("failed to parse length from {url}"));

    let number = chapter_number(&html)
        .unwrap_or_else(|_| panic!("failed to parse chapter number from {url}"));

    let (comments, replies, user_comments) = match comments::parse(id, number) {
        Ok(tup) => tup,

        Err(err) => {
            error!("Error: {err}, failed to parse comments from {url}");

            (
                0,
                0,
                vec![UserComment {
                    username: "".to_string(),
                    replies: 0,
                    upvotes: 0,
                    downvotes: 0,
                    contents: "".to_string(),
                    profile_type: "".to_string(),
                    auth_provider: "".to_string(),
                    country: "".to_string(),
                    post_date: "".to_string(),
                }],
            )
        }
    };

    let season = season_fn(&html, number);
    let season_chapter = season_chapter_fn(&html, number);
    let arc = arc_fn(&html, number);

    let result = Chapter {
        number: chapter,
        likes,
        length,
        comments,
        replies,
        season,
        season_chapter,
        arc,
        user_comments,
        published: None, // <---------- TODO: Find a way to get the published date
    };

    Some(result)
}

fn chapter_number(html: &Html) -> Result<u16> {
    if let Ok(chapter_number_selector) = Selector::parse("span.tx") {
        if let Some(chapter_number) = html.select(&chapter_number_selector).next() {
            // .ok_or_else(|| anyhow!("Should find a chapter number"))?
            let vec = chapter_number.text().collect::<Vec<_>>();

            let text = vec
                .first()
                .expect("Failed to select first occurrence of chapter number");

            let cleaned = text.replace('#', "");

            let result = cleaned
                .parse::<u16>()
                .with_context(|| format!("Failed to parse {cleaned} to a u16"))?;

            return Ok(result);
        }
    }

    bail!("Failed to create chapter number selector")
}

fn chapter_url(id: u32, chapter: u16) -> String {
    format!("https://www.webtoons.com/en/*/*/*/viewer?title_no={id}&episode_no={chapter}")
}

#[cfg(test)]
mod parse_comments_tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_parse_chapter_number() {
        const CHAPTER_NUMBER: &str =
            r##"<span class="tx _btnOpenEpisodeList NPI=a:current,g:en_en">#550</span>"##;

        let html = Html::parse_document(CHAPTER_NUMBER);

        let result = chapter_number(&html).unwrap();

        assert_eq!(result, 550);
    }

    #[test]
    fn should_form_valid_url() {
        const CHAPTER_NUMBER: &str =
            "https://www.webtoons.com/en/*/*/*/viewer?title_no=95&episode_no=2";

        let result = chapter_url(95, 2);

        assert_eq!(CHAPTER_NUMBER, result);
    }
}
