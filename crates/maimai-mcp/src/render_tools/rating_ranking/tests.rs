use std::path::PathBuf;

use maimai_app::rating_ranking::{RatingRankingImage, RatingRankingTarget, RatingRankingUsername};
use maimai_core::QqId;
use serde_json::json;

use super::{convert, dto::RatingRankingArgs, format};

#[test]
fn name_precedes_username_qq_range_and_page() -> Result<(), Box<dyn std::error::Error>> {
    let args: RatingRankingArgs = serde_json::from_value(json!({
        "name":" First ", "username":"second", "qq":"123456",
        "startRank":4, "endRank":7, "page":3
    }))?;
    let target = convert::target(args)?;
    assert_eq!(
        target,
        RatingRankingTarget::username(RatingRankingUsername::new("First")?)
    );
    Ok(())
}

#[test]
fn qq_precedes_range_and_zero_page_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let args: RatingRankingArgs =
        serde_json::from_value(json!({"qq":"00123","startRank":3,"endRank":4}))?;
    let target = convert::target(args)?;
    assert_eq!(target, RatingRankingTarget::qq(QqId::new("00123")?));

    for page in [0, -1] {
        let invalid: RatingRankingArgs = serde_json::from_value(json!({"page":page}))?;
        assert!(convert::target(invalid).is_err());
    }
    Ok(())
}

#[test]
fn range_is_bounded_and_output_is_text_json() -> Result<(), Box<dyn std::error::Error>> {
    let too_many: RatingRankingArgs = serde_json::from_value(json!({"startRank":1,"endRank":31}))?;
    assert_eq!(
        convert::target(too_many)
            .err()
            .ok_or("range should fail")?
            .to_string(),
        "Diving-Fish 公开排名一次最多输出 30 人。"
    );
    let too_long: RatingRankingArgs = serde_json::from_value(json!({
        "name":"x".repeat(maimai_app::rating_ranking::MAX_RANKING_USERNAME_CHARS + 1)
    }))?;
    let error = convert::target(too_long)
        .err()
        .ok_or("long username should fail")?;
    assert_eq!(error.to_string(), "name/username 最多 128 个字符");
    let text = format::image(&RatingRankingImage {
        image_path: PathBuf::from("/tmp/rating.png"),
        width: 320,
        height: 240,
    })?;
    assert_eq!(
        text,
        r#"{"imagePath":"/tmp/rating.png","mimeType":"image/png","width":320,"height":240}"#
    );
    Ok(())
}
