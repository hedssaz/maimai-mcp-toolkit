mod convert;
mod dto;
mod error;
mod output;

use maimai_core::scoring::{find_score_combinations, score_counts};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use convert::{score_count_request, search_request};
use dto::{FindCombinationsArgs, ScoreCountsArgs};
use error::AdapterError;
use output::{score_count_value, search_value};

pub(crate) fn score_counts_value(arguments: Map<String, Value>) -> Result<Value, AdapterError> {
    let arguments: ScoreCountsArgs = deserialize(arguments, "score_counts")?;
    let request = score_count_request(arguments)?;
    let result = score_counts(&request)?;
    Ok(score_count_value(&result))
}

pub(crate) fn find_combinations_value(
    arguments: Map<String, Value>,
) -> Result<Value, AdapterError> {
    let arguments: FindCombinationsArgs = deserialize(arguments, "find_score_combinations")?;
    let request = search_request(arguments)?;
    let result = find_score_combinations(&request)?;
    search_value(&request, &result)
}

fn deserialize<T: DeserializeOwned>(
    arguments: Map<String, Value>,
    tool: &str,
) -> Result<T, AdapterError> {
    serde_json::from_value(Value::Object(arguments))
        .map_err(|error| AdapterError::input(format!("Invalid arguments for tool {tool}: {error}")))
}

#[cfg(test)]
mod tests;
