//! Input checks shared by the handlers. Each returns a 400 with a message
//! that names the field.

use crate::error::{AppError, AppResult};
use foyer_types::{Media, PageStyle, SupportLink};

pub const MAX_TAGS: usize = 10;
pub const MAX_SUPPORT_LINKS: usize = 10;

/// Lowercase letters, digits and inner `-`, `min..=max` long.
pub fn slug(field: &str, value: &str, min: usize, max: usize) -> AppResult<()> {
    let ok = (min..=max).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !value.starts_with('-')
        && !value.ends_with('-')
        && !value.contains("--");
    if ok {
        Ok(())
    } else {
        Err(AppError::validation(format!(
            "{field} must be {min} to {max} lowercase letters, digits or single dashes, \
             not starting or ending with a dash"
        )))
    }
}

pub fn username(value: &str) -> AppResult<()> {
    let ok = (3..=32).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
    if ok {
        Ok(())
    } else {
        Err(AppError::validation(
            "username must be 3 to 32 lowercase letters, digits, - or _",
        ))
    }
}

pub fn email(value: &str) -> AppResult<()> {
    let ok = value.len() <= 254
        && !value.chars().any(char::is_whitespace)
        && value
            .split_once('@')
            .is_some_and(|(local, domain)| !local.is_empty() && domain.contains('.'));
    if ok {
        Ok(())
    } else {
        Err(AppError::validation("email is not a valid address"))
    }
}

pub fn password(value: &str) -> AppResult<()> {
    if (10..=1024).contains(&value.chars().count()) {
        Ok(())
    } else {
        Err(AppError::validation(
            "password must be 10 to 1024 characters",
        ))
    }
}

/// Non-empty after trimming and at most `max` characters.
pub fn required_text(field: &str, value: &str, max: usize) -> AppResult<()> {
    if value.trim().is_empty() {
        return Err(AppError::validation(format!("{field} must not be empty")));
    }
    text(field, value, max)
}

pub fn text(field: &str, value: &str, max: usize) -> AppResult<()> {
    if value.chars().count() > max {
        Err(AppError::validation(format!(
            "{field} must be at most {max} characters"
        )))
    } else {
        Ok(())
    }
}

/// An absolute `https://` URL.
pub fn https_url(field: &str, value: &str) -> AppResult<()> {
    let ok = value.len() <= 2048
        && url::Url::parse(value).is_ok_and(|u| u.scheme() == "https" && u.host().is_some());
    if ok {
        Ok(())
    } else {
        Err(AppError::validation(format!(
            "{field} must be an https:// URL"
        )))
    }
}

pub fn support_links(links: &[SupportLink]) -> AppResult<()> {
    if links.len() > MAX_SUPPORT_LINKS {
        return Err(AppError::validation(format!(
            "at most {MAX_SUPPORT_LINKS} support links"
        )));
    }
    for link in links {
        https_url("support_links.url", &link.url)?;
        if let Some(label) = &link.label {
            text("support_links.label", label, 60)?;
        }
    }
    Ok(())
}

/// Normalizes tags (trim, lowercase, dedupe) and checks them.
pub fn tags(tags: &[String]) -> AppResult<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for tag in tags {
        let tag = tag.trim().to_lowercase();
        slug("tag", &tag, 2, 30)?;
        if !out.contains(&tag) {
            out.push(tag);
        }
    }
    if out.len() > MAX_TAGS {
        return Err(AppError::validation(format!("at most {MAX_TAGS} tags")));
    }
    Ok(out)
}

/// Normalizes a page design and checks it against the game's images. The
/// rules (readable colours, known images, YouTube or Vimeo trailers) live in
/// `foyer_types::PageStyle::check`, shared with the website.
pub fn page_style(mut style: PageStyle, media: &[Media]) -> AppResult<PageStyle> {
    style.normalize();
    style
        .check(media)
        .map_err(|e| AppError::validation(format!("page.{e}")))?;
    Ok(style)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert!(slug("slug", "my-game-2", 2, 40).is_ok());
        for bad in [
            "a", "-game", "game-", "my--game", "My-Game", "game_1", "spa ce",
        ] {
            assert!(
                slug("slug", bad, 2, 40).is_err(),
                "{bad} should be rejected"
            );
        }
    }

    #[test]
    fn emails_and_urls() {
        assert!(email("dev@example.com").is_ok());
        assert!(email("dev@localhost").is_err());
        assert!(email("no-at.example.com").is_err());
        assert!(https_url("url", "https://ko-fi.com/dev").is_ok());
        assert!(https_url("url", "http://ko-fi.com/dev").is_err());
        assert!(https_url("url", "javascript:alert(1)").is_err());
    }

    #[test]
    fn tags_are_normalized() {
        let tags = tags(&["Roguelike".into(), " roguelike ".into(), "pixel-art".into()]).unwrap();
        assert_eq!(tags, ["roguelike", "pixel-art"]);
    }
}
