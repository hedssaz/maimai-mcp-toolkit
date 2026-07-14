#!/usr/bin/env python3
"""Standalone stdio MCP server for maimai DX scoring searches."""

from __future__ import annotations

import argparse
import json
import math
import sys
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import dataclass
from decimal import Decimal, ROUND_FLOOR, ROUND_HALF_UP
from fractions import Fraction
from typing import Any


NOTE_TYPES = ("tap", "touch", "hold", "slide", "break")
JUDGMENTS = (
    "miss",
    "good",
    "great_low",
    "great_mid",
    "great_high",
    "perfect_low",
    "perfect_high",
    "critical",
)

BASE_VALUES = {
    "tap": {
        "miss": 0,
        "good": 250,
        "great_low": 400,
        "great_mid": 400,
        "great_high": 400,
        "perfect_low": 500,
        "perfect_high": 500,
        "critical": 500,
    },
    "touch": {
        "miss": 0,
        "good": 250,
        "great_low": 400,
        "great_mid": 400,
        "great_high": 400,
        "perfect_low": 500,
        "perfect_high": 500,
        "critical": 500,
    },
    "hold": {
        "miss": 0,
        "good": 500,
        "great_low": 800,
        "great_mid": 800,
        "great_high": 800,
        "perfect_low": 1000,
        "perfect_high": 1000,
        "critical": 1000,
    },
    "slide": {
        "miss": 0,
        "good": 750,
        "great_low": 1200,
        "great_mid": 1200,
        "great_high": 1200,
        "perfect_low": 1500,
        "perfect_high": 1500,
        "critical": 1500,
    },
    "break": {
        "miss": 0,
        "good": 1000,
        "great_low": 1250,
        "great_mid": 1500,
        "great_high": 2000,
        "perfect_low": 2500,
        "perfect_high": 2500,
        "critical": 2500,
    },
}

BREAK_BONUS_VALUES = {
    "miss": 0,
    "good": 30,
    "great_low": 40,
    "great_mid": 40,
    "great_high": 40,
    "perfect_low": 50,
    "perfect_high": 75,
    "critical": 100,
}

OLD_SCORE_VALUES = {
    "tap": BASE_VALUES["tap"],
    "touch": BASE_VALUES["touch"],
    "hold": BASE_VALUES["hold"],
    "slide": BASE_VALUES["slide"],
    "break": {
        "miss": 0,
        "good": 1000,
        "great_low": 1250,
        "great_mid": 1500,
        "great_high": 2000,
        "perfect_low": 2500,
        "perfect_high": 2550,
        "critical": 2600,
    },
}

DX_VALUES = {
    "miss": 0,
    "good": 0,
    "great_low": 1,
    "great_mid": 1,
    "great_high": 1,
    "perfect_low": 2,
    "perfect_high": 2,
    "critical": 3,
}

COUNT_ALIASES = {
    "m": "miss",
    "miss": "miss",
    "good": "good",
    "g": "good",
    "great_low": "great_low",
    "great_mid": "great_mid",
    "great_high": "great_high",
    "perfect_low": "perfect_low",
    "perfect_high": "perfect_high",
    "critical": "critical",
    "critical_perfect": "critical",
    "cp": "critical",
}

GROUP_ALIASES = {
    "all": set(JUDGMENTS),
    "any": set(JUDGMENTS),
    "not_miss": set(JUDGMENTS) - {"miss"},
    "no_miss": set(JUDGMENTS) - {"miss"},
    "great": {"great_low", "great_mid", "great_high"},
    "perfect": {"perfect_low", "perfect_high"},
    "perfect_or_critical": {"perfect_low", "perfect_high", "critical"},
    "ap": {"perfect_low", "perfect_high", "critical"},
    "fc_plus": {"great_low", "great_mid", "great_high", "perfect_low", "perfect_high", "critical"},
    "fc": set(JUDGMENTS) - {"miss"},
}

RAW_SCORE_MODES = ("base", "break_bonus", "oldscore", "dxscore")
ACC_SCORE_MODES = ("oldacc", "dxacc")
SCORE_MODES = (*RAW_SCORE_MODES, *ACC_SCORE_MODES)
FIND_SCORE_MODES = SCORE_MODES
SCORE_MODE_ALIASES = {
    "old_score": "oldscore",
    "finale": "oldscore",
    "dx_score": "dxscore",
    "dxstar": "dxscore",
    "dxstars": "dxscore",
    "star": "dxscore",
    "stars": "dxscore",
    "dx_achievement": "dxacc",
    "dx_acc": "dxacc",
    "acc": "dxacc",
    "old_acc": "oldacc",
    "old_achievement": "oldacc",
    "old_achievement_rate": "oldacc",
    "old_percent": "oldacc",
    "old_percentage": "oldacc",
}
ACHIEVEMENT_DISPLAY_MODES = ("floor", "half_up", "exact")


class MaimaiError(ValueError):
    """User-facing validation error."""


@dataclass(frozen=True)
class NoteTypePlan:
    note_type: str
    judgments: tuple[str, ...]
    base_counts: dict[str, int]
    remaining: int
    max_remaining: dict[str, int]
    fixed_score: int


def clean_name(value: str) -> str:
    return value.strip().lower().replace(" ", "_").replace("-", "_")


def normalize_score_mode(value: Any, allowed: tuple[str, ...] = SCORE_MODES) -> str:
    score_mode = clean_name(str(value))
    score_mode = SCORE_MODE_ALIASES.get(score_mode, score_mode)
    if score_mode not in allowed:
        raise MaimaiError(f"score_mode must be one of: {', '.join(allowed)}")
    return score_mode


def normalize_note_type(value: str) -> str:
    note_type = clean_name(value)
    if note_type in {"tch", "touch_note"}:
        note_type = "touch"
    if note_type in {"touchhold", "touch_hold", "touchh", "touch_hold_note", "thold"}:
        note_type = "hold"
    if note_type in {"brk", "break_note"}:
        note_type = "break"
    if note_type not in NOTE_TYPES:
        raise MaimaiError(f"unknown note type: {value!r}")
    return note_type


def normalize_count_judgment(value: str, note_type: str | None = None) -> str:
    key = clean_name(value)
    if key in {"perfect", "great"}:
        if note_type == "break":
            raise MaimaiError(
                f"break.{key} is ambiguous; use explicit break judgments "
                "great_low/great_mid/great_high or perfect_low/perfect_high"
            )
        return "perfect_high" if key == "perfect" else "great_mid"
    if key not in COUNT_ALIASES:
        raise MaimaiError(f"unknown judgment: {value!r}")
    return COUNT_ALIASES[key]


def display_judgment(note_type: str, judgment: str) -> str:
    if note_type != "break":
        if judgment in {"great_low", "great_mid", "great_high"}:
            return "great"
        if judgment in {"perfect_low", "perfect_high"}:
            return "perfect"
    return judgment


def display_judgments(note_type: str, judgments: set[str]) -> list[str]:
    return sorted({display_judgment(note_type, judgment) for judgment in judgments})


def expand_judgment_token(value: str) -> set[str]:
    key = clean_name(value)
    if key in GROUP_ALIASES:
        return set(GROUP_ALIASES[key])
    if key in COUNT_ALIASES:
        return {COUNT_ALIASES[key]}
    raise MaimaiError(f"unknown judgment or group: {value!r}")


def expand_judgment_list(values: Any) -> set[str]:
    if values is None:
        return set(JUDGMENTS)
    if isinstance(values, str):
        values = [values]
    if not isinstance(values, list):
        raise MaimaiError("judgment restrictions must be strings or string arrays")
    expanded: set[str] = set()
    for value in values:
        if not isinstance(value, str):
            raise MaimaiError("judgment restrictions must contain only strings")
        expanded.update(expand_judgment_token(value))
    return expanded


def normalize_count_map(raw_counts: Any) -> dict[str, dict[str, int]]:
    if raw_counts is None:
        return {note_type: {} for note_type in NOTE_TYPES}
    if not isinstance(raw_counts, dict):
        raise MaimaiError("counts must be an object")

    result = {note_type: {judgment: 0 for judgment in JUDGMENTS} for note_type in NOTE_TYPES}
    for raw_note_type, raw_judgments in raw_counts.items():
        note_type = normalize_note_type(str(raw_note_type))
        if not isinstance(raw_judgments, dict):
            raise MaimaiError(f"counts.{raw_note_type} must be an object")
        for raw_judgment, raw_count in raw_judgments.items():
            judgment = normalize_count_judgment(str(raw_judgment), note_type)
            count = parse_non_negative_int(raw_count, f"counts.{raw_note_type}.{raw_judgment}")
            result[note_type][judgment] += count
    return result


def parse_non_negative_int(value: Any, name: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise MaimaiError(f"{name} must be a non-negative integer")
    if value < 0:
        raise MaimaiError(f"{name} must be a non-negative integer")
    return value


def parse_optional_int(value: Any, name: str) -> int | None:
    if value is None:
        return None
    return parse_non_negative_int(value, name)


def parse_non_negative_int_like(value: Any, name: str) -> int:
    if isinstance(value, bool) or value is None:
        raise MaimaiError(f"{name} must be a non-negative integer")
    if isinstance(value, int):
        result = value
    elif isinstance(value, str) and value.strip().isdigit():
        result = int(value.strip())
    else:
        raise MaimaiError(f"{name} must be a non-negative integer")
    if result < 0:
        raise MaimaiError(f"{name} must be a non-negative integer")
    return result


def parse_optional_int_like(value: Any, name: str) -> int | None:
    if value is None:
        return None
    return parse_non_negative_int_like(value, name)


def parse_fraction_decimal(value: Any, name: str) -> Fraction:
    if isinstance(value, bool) or value is None:
        raise MaimaiError(f"{name} must be a decimal number")
    try:
        decimal_value = Decimal(str(value).strip())
    except Exception as exc:
        raise MaimaiError(f"{name} must be a decimal number") from exc
    if not decimal_value.is_finite():
        raise MaimaiError(f"{name} must be a finite decimal number")
    return Fraction(decimal_value)


def fraction_to_decimal(value: Fraction, places: int = 12) -> Decimal:
    quant = Decimal(1).scaleb(-places)
    decimal_value = Decimal(value.numerator) / Decimal(value.denominator)
    return decimal_value.quantize(quant)


def format_fraction_decimal(value: Fraction, places: int = 12) -> str:
    text = format(fraction_to_decimal(value, places), "f")
    if "." in text:
        text = text.rstrip("0").rstrip(".")
    return text or "0"


def format_display_achievement(value: Fraction, display_mode: str, digits: int) -> str:
    quant = Decimal(1).scaleb(-digits)
    decimal_value = Decimal(value.numerator) / Decimal(value.denominator)
    if display_mode == "half_up":
        rounded = decimal_value.quantize(quant, rounding=ROUND_HALF_UP)
    else:
        rounded = decimal_value.quantize(quant, rounding=ROUND_FLOOR)
    return f"{rounded:.{digits}f}"


def parse_percent_decimal(value: Any, name: str) -> Fraction:
    if isinstance(value, str):
        value = value.strip()
        if value.endswith("%"):
            value = value[:-1].strip()
    return parse_fraction_decimal(value, name)


def max_base_score(note_totals: dict[str, int]) -> int:
    return (
        (note_totals["tap"] + note_totals["touch"]) * 500
        + note_totals["hold"] * 1000
        + note_totals["slide"] * 1500
        + note_totals["break"] * 2500
    )


def max_old_score(note_totals: dict[str, int]) -> int:
    return (
        (note_totals["tap"] + note_totals["touch"]) * 500
        + note_totals["hold"] * 1000
        + note_totals["slide"] * 1500
        + note_totals["break"] * 2600
    )


def percent_details_from_fraction(value: Fraction, display_digits: int) -> dict[str, Any]:
    floor_text = format_display_achievement(value, "floor", display_digits)
    half_up_text = format_display_achievement(value, "half_up", display_digits)
    scale = 10**display_digits
    return {
        "raw": format_fraction_decimal(value),
        "display_floor": floor_text,
        "display_half_up": half_up_text,
        "scaled_floor": int(Decimal(floor_text) * scale),
        "scaled_half_up": int(Decimal(half_up_text) * scale),
    }


def oldacc_details_from_score(
    old_score: int,
    note_totals: dict[str, int],
    display_digits: int = 4,
) -> dict[str, Any] | None:
    denominator = max_base_score(note_totals)
    if denominator <= 0:
        return None
    return percent_details_from_fraction(Fraction(old_score * 100, denominator), display_digits)


def dxacc_details_from_scores(
    base_score: int,
    break_bonus_score: int,
    note_totals: dict[str, int],
    display_digits: int = 4,
) -> dict[str, Any] | None:
    max_base = max_base_score(note_totals)
    if max_base <= 0:
        return None
    value = Fraction(base_score * 100, max_base)
    max_break_bonus = note_totals["break"] * 100
    if max_break_bonus:
        value += Fraction(break_bonus_score, max_break_bonus)
    return percent_details_from_fraction(value, display_digits)


def ceil_fraction(value: Fraction) -> int:
    return -(-value.numerator // value.denominator)


def floor_fraction(value: Fraction) -> int:
    return value.numerator // value.denominator


def score_value(note_type: str, judgment: str, score_mode: str) -> int:
    score_mode = normalize_score_mode(score_mode, RAW_SCORE_MODES)
    if score_mode == "base":
        return BASE_VALUES[note_type][judgment]
    if score_mode == "break_bonus":
        return BREAK_BONUS_VALUES[judgment] if note_type == "break" else 0
    if score_mode == "oldscore":
        return OLD_SCORE_VALUES[note_type][judgment]
    if score_mode == "dxscore":
        return DX_VALUES[judgment]
    raise MaimaiError(f"unknown score_mode: {score_mode!r}")


def score_counts(arguments: dict[str, Any]) -> dict[str, Any]:
    score_mode_provided = arguments.get("score_mode") is not None
    score_mode = normalize_score_mode(arguments["score_mode"]) if score_mode_provided else "oldscore"
    display_digits = parse_non_negative_int_like(arguments.get("display_digits", 4), "display_digits")
    if display_digits <= 0 or display_digits > 8:
        raise MaimaiError("display_digits must be between 1 and 8")

    counts = normalize_count_map(arguments.get("counts", {}))
    include_zero = bool(arguments.get("include_zero", False))

    row_map: dict[tuple[str, str], dict[str, Any]] = {}
    totals = {
        "base": 0,
        "break_bonus": 0,
        "oldscore": 0,
        "dxscore": 0,
    }
    selected_mode_total = 0

    for note_type in NOTE_TYPES:
        for judgment in JUDGMENTS:
            count = counts[note_type][judgment]
            base = BASE_VALUES[note_type][judgment] * count
            break_bonus = BREAK_BONUS_VALUES[judgment] * count if note_type == "break" else 0
            oldscore = OLD_SCORE_VALUES[note_type][judgment] * count
            dxscore = DX_VALUES[judgment] * count
            selected = (
                score_value(note_type, judgment, score_mode) * count
                if score_mode_provided and score_mode in RAW_SCORE_MODES
                else None
            )
            totals["base"] += base
            totals["break_bonus"] += break_bonus
            totals["oldscore"] += oldscore
            totals["dxscore"] += dxscore
            if selected is not None:
                selected_mode_total += selected
            if include_zero or count:
                judgment_name = display_judgment(note_type, judgment)
                contribution = {
                    "base": base,
                    "break_bonus": break_bonus,
                    "oldscore": oldscore,
                    "dxscore": dxscore,
                }
                if selected is not None:
                    contribution["selected_mode"] = selected
                row_key = (note_type, judgment_name)
                row = row_map.get(row_key)
                if row is None:
                    row = {
                        "note_type": note_type,
                        "judgment": judgment_name,
                        "count": 0,
                        "per_note": {
                            "base": BASE_VALUES[note_type][judgment],
                            "break_bonus": BREAK_BONUS_VALUES[judgment] if note_type == "break" else 0,
                            "oldscore": score_value(note_type, judgment, "oldscore"),
                            "dxscore": DX_VALUES[judgment],
                        },
                        "contribution": {key: 0 for key in contribution},
                    }
                    row_map[row_key] = row
                row["count"] += count
                for key, value in contribution.items():
                    row["contribution"][key] = row["contribution"].get(key, 0) + value

    note_totals = {
        note_type: sum(counts[note_type].values())
        for note_type in NOTE_TYPES
    }
    totals["oldacc"] = oldacc_details_from_score(totals["oldscore"], note_totals, display_digits)
    totals["dxacc"] = dxacc_details_from_scores(totals["base"], totals["break_bonus"], note_totals, display_digits)
    if score_mode_provided:
        totals["selected_mode"] = (
            selected_mode_total if score_mode in RAW_SCORE_MODES else totals[score_mode]
        )

    result = {
        "display_digits": display_digits,
        "note_totals": note_totals,
        "rows": list(row_map.values()),
        "totals": totals,
        "judgment_groups": judgment_group_reference(),
    }
    if score_mode_provided:
        result["score_mode"] = score_mode
    return result


def judgment_group_reference() -> dict[str, list[str]]:
    return {key: sorted(value) for key, value in sorted(GROUP_ALIASES.items())}


def normalize_nested_counts(raw: Any, name: str) -> dict[str, dict[str, int]]:
    if raw is None:
        return {note_type: {} for note_type in NOTE_TYPES}
    if not isinstance(raw, dict):
        raise MaimaiError(f"{name} must be an object")
    result = {note_type: {} for note_type in NOTE_TYPES}
    for raw_note_type, raw_judgments in raw.items():
        note_type = normalize_note_type(str(raw_note_type))
        if not isinstance(raw_judgments, dict):
            raise MaimaiError(f"{name}.{raw_note_type} must be an object")
        for raw_judgment, raw_count in raw_judgments.items():
            judgment = normalize_count_judgment(str(raw_judgment), note_type)
            count = parse_non_negative_int(raw_count, f"{name}.{raw_note_type}.{raw_judgment}")
            result[note_type][judgment] = result[note_type].get(judgment, 0) + count
    return result


def normalize_note_totals(raw: Any) -> dict[str, int]:
    if not isinstance(raw, dict):
        raise MaimaiError("note_totals is required and must be an object")
    result = {note_type: 0 for note_type in NOTE_TYPES}
    for raw_note_type, raw_count in raw.items():
        note_type = normalize_note_type(str(raw_note_type))
        result[note_type] += parse_non_negative_int(raw_count, f"note_totals.{raw_note_type}")
    if sum(result.values()) == 0:
        raise MaimaiError("note_totals must contain at least one note")
    return result


def normalize_restriction_map(raw: Any, name: str, default_all: bool) -> dict[str, set[str]]:
    result = {note_type: set(JUDGMENTS) if default_all else set() for note_type in NOTE_TYPES}
    if raw is None:
        return result
    if not isinstance(raw, dict):
        raise MaimaiError(f"{name} must be an object")
    for raw_note_type, raw_values in raw.items():
        note_type = normalize_note_type(str(raw_note_type))
        result[note_type] = expand_judgment_list(raw_values)
    return result


def build_note_type_plan(
    note_type: str,
    total_count: int,
    score_mode: str,
    allowed: set[str],
    fixed_counts: dict[str, int],
    min_counts: dict[str, int],
    max_counts: dict[str, int],
) -> NoteTypePlan:
    if total_count == 0:
        return NoteTypePlan(note_type, tuple(), {}, 0, {}, 0)

    fixed = dict(fixed_counts)
    minimums = dict(min_counts)
    maximums = dict(max_counts)

    for judgment in set(fixed) | set(minimums) | set(maximums):
        if judgment not in allowed:
            raise MaimaiError(f"{note_type}.{judgment} is constrained but not allowed")

    base_counts = {judgment: 0 for judgment in JUDGMENTS}
    exact_judgments: set[str] = set()

    for judgment, fixed_count in fixed.items():
        if judgment in minimums and fixed_count < minimums[judgment]:
            raise MaimaiError(f"{note_type}.{judgment} fixed count is below its minimum")
        if judgment in maximums and fixed_count > maximums[judgment]:
            raise MaimaiError(f"{note_type}.{judgment} fixed count is above its maximum")
        base_counts[judgment] = fixed_count
        exact_judgments.add(judgment)

    for judgment, minimum in minimums.items():
        if judgment in exact_judgments:
            continue
        if judgment in maximums and minimum > maximums[judgment]:
            raise MaimaiError(f"{note_type}.{judgment} minimum is above maximum")
        base_counts[judgment] += minimum

    used = sum(base_counts.values())
    if used > total_count:
        raise MaimaiError(f"{note_type} constraints use {used} notes, above total {total_count}")

    remaining = total_count - used
    max_remaining: dict[str, int] = {}
    judgments = tuple(judgment for judgment in JUDGMENTS if judgment in allowed)
    if not judgments and remaining:
        raise MaimaiError(f"{note_type} has notes left but no allowed judgments")

    for judgment in judgments:
        if judgment in exact_judgments:
            cap = 0
        elif judgment in maximums:
            cap = maximums[judgment] - base_counts[judgment]
        else:
            cap = remaining
        if cap < 0:
            raise MaimaiError(f"{note_type}.{judgment} maximum is below required count")
        max_remaining[judgment] = cap

    if sum(max_remaining.values()) < remaining:
        raise MaimaiError(f"{note_type} max constraints cannot fill {remaining} remaining notes")

    fixed_score = sum(
        score_value(note_type, judgment, score_mode) * count
        for judgment, count in base_counts.items()
    )
    return NoteTypePlan(note_type, judgments, base_counts, remaining, max_remaining, fixed_score)


def count_state_entries(states: dict[Any, list[Any]]) -> int:
    return sum(len(samples) for samples in states.values())


def append_limited(samples: list[Any], sample: Any, limit: int) -> bool:
    if len(samples) < limit:
        samples.append(sample)
        return True
    return False


def possible_scores_for_plan(
    plan: NoteTypePlan,
    score_mode: str,
    max_states: int,
    max_samples_per_score: int,
    score_values: dict[str, int] | None = None,
    score_cap: int | None = None,
) -> tuple[dict[int, list[dict[str, int]]], bool]:
    if plan.remaining == 0:
        return {plan.fixed_score: [compact_counts(plan.base_counts)]}, False

    judgment_values = {
        judgment: score_values[judgment] if score_values is not None else score_value(plan.note_type, judgment, score_mode)
        for judgment in plan.judgments
    }
    zero_group = tuple(
        judgment
        for judgment in plan.judgments
        if judgment_values[judgment] == 0 and plan.max_remaining.get(judgment, 0) > 0
    )
    grouped: dict[int, list[str]] = {}
    for judgment in plan.judgments:
        if judgment in zero_group:
            continue
        if plan.max_remaining.get(judgment, 0) <= 0:
            continue
        grouped.setdefault(judgment_values[judgment], []).append(judgment)
    search_groups = tuple((value, tuple(judgments)) for value, judgments in grouped.items())
    zero_capacity = sum(plan.max_remaining[judgment] for judgment in zero_group)

    states: dict[tuple[int, int], list[tuple[int, ...]]] = {(0, 0): [tuple()]}
    truncated = False

    for group_index, (value, judgments) in enumerate(search_groups):
        cap = min(sum(plan.max_remaining[judgment] for judgment in judgments), plan.remaining)
        next_states: dict[tuple[int, int], list[tuple[int, ...]]] = {}
        for (used, score), count_samples in states.items():
            max_count = min(cap, plan.remaining - used)
            for count in range(max_count + 1):
                for counts_tuple in count_samples:
                    new_used = used + count
                    new_score = score + value * count
                    if score_cap is not None and new_score > score_cap:
                        continue
                    new_counts = (*counts_tuple, count)
                    key = (new_used, new_score)
                    if not append_limited(
                        next_states.setdefault(key, []),
                        new_counts,
                        max_samples_per_score,
                    ):
                        truncated = True
            if count_state_entries(next_states) > max_states:
                raise MaimaiError(
                    f"search exceeded max_states={max_states} while solving {plan.note_type}. "
                    "Narrow the search with allowed_judgments, disallowed_judgments, "
                    "fixed_counts, min_counts, or max_counts before retrying. "
                    "For DX achievement percentages, use score_mode=dxacc and pass "
                    "target_score as the displayed percentage without the decimal point, "
                    "or pass a percentage string such as 100.4999%."
                )
        states = next_states

    results: dict[int, list[dict[str, int]]] = {}
    for (used, score), count_samples in states.items():
        zero_fill = plan.remaining - used
        if zero_fill < 0:
            continue
        if zero_group:
            if zero_fill > zero_capacity:
                continue
        elif used != plan.remaining:
            continue
        total_score = plan.fixed_score + score
        score_samples = results.setdefault(total_score, [])
        for counts_tuple in count_samples:
            counts = dict(plan.base_counts)
            for (_, judgments), count in zip(search_groups, counts_tuple, strict=True):
                fill_left = count
                for judgment in judgments:
                    if fill_left <= 0:
                        break
                    fill = min(fill_left, plan.max_remaining[judgment])
                    counts[judgment] += fill
                    fill_left -= fill
            fill_left = zero_fill
            for judgment in zero_group:
                if fill_left <= 0:
                    break
                fill = min(fill_left, plan.max_remaining[judgment])
                counts[judgment] += fill
                fill_left -= fill
            if not append_limited(score_samples, compact_counts(counts), max_samples_per_score):
                truncated = True
    return results, truncated


def compact_counts(counts: dict[str, int], note_type: str | None = None) -> dict[str, int]:
    compacted: dict[str, int] = {}
    for judgment, count in counts.items():
        if not count:
            continue
        display_name = display_judgment(note_type, judgment) if note_type is not None else judgment
        compacted[display_name] = compacted.get(display_name, 0) + count
    return compacted


def parse_acc_value(value: Any, name: str, display_digits: int) -> Fraction:
    if isinstance(value, bool) or value is None:
        raise MaimaiError(f"{name} must be a percentage or scaled integer")
    if isinstance(value, int):
        return Fraction(value, 10**display_digits)
    if isinstance(value, str):
        text = value.strip()
        if text.endswith("%"):
            return parse_percent_decimal(text, name)
        if "." in text:
            return parse_percent_decimal(text, name)
        if text.isdigit():
            return Fraction(int(text), 10**display_digits)
        return parse_percent_decimal(text, name)
    return parse_percent_decimal(value, name)


def first_present(arguments: dict[str, Any], keys: tuple[str, ...]) -> tuple[str, Any] | None:
    for key in keys:
        if arguments.get(key) is not None:
            return key, arguments[key]
    return None


def parse_optional_bool_alias(arguments: dict[str, Any], keys: tuple[str, ...]) -> bool:
    values = [(key, arguments[key]) for key in keys if arguments.get(key) is not None]
    if not values:
        return False
    parsed_values: list[bool] = []
    for key, value in values:
        if isinstance(value, bool):
            parsed_values.append(value)
            continue
        if isinstance(value, str):
            text = clean_name(value)
            if text in {"true", "yes", "y", "1", "on"}:
                parsed_values.append(True)
                continue
            if text in {"false", "no", "n", "0", "off"}:
                parsed_values.append(False)
                continue
        raise MaimaiError(f"{key} must be a boolean")
    if any(value != parsed_values[0] for value in parsed_values):
        raise MaimaiError(f"{'/'.join(keys)} aliases disagree")
    return parsed_values[0]


def parse_optional_int_alias(arguments: dict[str, Any], keys: tuple[str, ...]) -> int | None:
    values = [
        (key, parse_non_negative_int_like(arguments[key], key))
        for key in keys
        if arguments.get(key) is not None
    ]
    if not values:
        return None
    first = values[0][1]
    if any(value != first for _, value in values):
        raise MaimaiError(f"{'/'.join(keys)} aliases disagree")
    return first


def apply_judgment_shortcuts(
    arguments: dict[str, Any],
    note_totals: dict[str, int],
    allowed: dict[str, set[str]],
    min_counts: dict[str, dict[str, int]],
) -> dict[str, Any]:
    shortcuts: dict[str, Any] = {}

    no_miss_good = parse_optional_bool_alias(
        arguments,
        (
            "no_miss_good",
            "all_notes_no_miss_good",
            "fc_plus_only",
            "no_good_miss",
        ),
    )
    if no_miss_good:
        for note_type in NOTE_TYPES:
            allowed[note_type] -= {"miss", "good"}
        shortcuts["no_miss_good"] = True

    break_cap = parse_optional_int_alias(
        arguments,
        (
            "break_max_perfect_or_below",
            "break_max_perfect_or_lower",
            "break_max_below_critical",
            "max_break_below_critical",
            "break_max_non_critical",
            "max_break_non_critical",
        ),
    )
    if break_cap is not None:
        required_critical = max(0, note_totals["break"] - break_cap)
        if required_critical:
            min_counts["break"]["critical"] = max(
                min_counts["break"].get("critical", 0),
                required_critical,
            )
        shortcuts["break_max_perfect_or_below"] = break_cap
        shortcuts["break_min_critical"] = required_critical

    return shortcuts


def target_acc_fraction(arguments: dict[str, Any], display_digits: int) -> Fraction | None:
    acc_target = first_present(
        arguments,
        (
            "target_acc",
            "target_percent",
            "target_percentage",
            "target_dxacc",
            "target_oldacc",
        ),
    )
    if acc_target is not None:
        key, value = acc_target
        return parse_percent_decimal(value, key)
    if arguments.get("target_score") is not None:
        return parse_acc_value(arguments["target_score"], "target_score", display_digits)
    return None


def acc_bound_fraction(
    arguments: dict[str, Any],
    score_key: str,
    percent_keys: tuple[str, ...],
    display_digits: int,
) -> Fraction | None:
    if arguments.get(score_key) is not None:
        return parse_acc_value(arguments[score_key], score_key, display_digits)
    percent_value = first_present(arguments, percent_keys)
    if percent_value is None:
        return None
    key, value = percent_value
    return parse_percent_decimal(value, key)


def find_score_combinations(arguments: dict[str, Any]) -> dict[str, Any]:
    score_mode = normalize_score_mode(arguments.get("score_mode", "oldscore"), FIND_SCORE_MODES)
    if score_mode in ACC_SCORE_MODES:
        return find_acc_combinations(arguments, score_mode)
    return find_raw_score_combinations(arguments, score_mode)


def find_raw_score_combinations(arguments: dict[str, Any], score_mode: str) -> dict[str, Any]:
    note_totals = normalize_note_totals(arguments.get("note_totals"))
    target_score = parse_optional_int_like(arguments.get("target_score"), "target_score")
    min_score = parse_optional_int_like(arguments.get("min_score"), "min_score")
    max_score = parse_optional_int_like(arguments.get("max_score"), "max_score")

    if target_score is not None:
        if min_score is not None or max_score is not None:
            raise MaimaiError("use either target_score or min_score/max_score, not both")
        min_score = target_score
        max_score = target_score
    if min_score is None and max_score is None:
        raise MaimaiError("target_score or min_score/max_score is required")
    if min_score is None:
        min_score = 0
    if max_score is None:
        max_score = 10**18
    if min_score > max_score:
        raise MaimaiError("min_score cannot be greater than max_score")

    allowed = normalize_restriction_map(arguments.get("allowed_judgments"), "allowed_judgments", True)
    disallowed = normalize_restriction_map(arguments.get("disallowed_judgments"), "disallowed_judgments", False)
    for note_type in NOTE_TYPES:
        allowed[note_type] -= disallowed[note_type]

    fixed_counts = normalize_nested_counts(arguments.get("fixed_counts"), "fixed_counts")
    min_counts = normalize_nested_counts(arguments.get("min_counts"), "min_counts")
    max_counts = normalize_nested_counts(arguments.get("max_counts"), "max_counts")
    shortcut_constraints = apply_judgment_shortcuts(arguments, note_totals, allowed, min_counts)

    max_solutions = parse_non_negative_int_like(arguments.get("max_solutions", 10), "max_solutions")
    max_states = parse_non_negative_int_like(arguments.get("max_states", 200000), "max_states")
    if max_solutions == 0:
        max_solutions = 1
    if max_states == 0:
        raise MaimaiError("max_states must be greater than zero")

    per_type_scores: list[tuple[str, dict[int, list[dict[str, int]]]]] = []
    per_type_summary = []
    truncated = False

    with ThreadPoolExecutor(max_workers=5) as executor:
        future_map: dict[Any, str] = {}
        for note_type in NOTE_TYPES:
            future = executor.submit(
                possible_scores_for_plan,
                build_note_type_plan(
                    note_type,
                    note_totals[note_type],
                    score_mode,
                    allowed[note_type],
                    fixed_counts[note_type],
                    min_counts[note_type],
                    max_counts[note_type],
                ),
                score_mode,
                max_states,
                max_solutions,
            )
            future_map[future] = note_type
        for future in as_completed(future_map):
            note_type = future_map[future]
            score_map, score_map_truncated = future.result()
            truncated = truncated or score_map_truncated
            if note_totals[note_type] and not score_map:
                raise MaimaiError(f"{note_type} has no possible scores under the constraints")
            per_type_scores.append((note_type, score_map))
            if score_map:
                per_type_summary.append(
                    {
                        "note_type": note_type,
                        "possible_score_count": len(score_map),
                        "sample_combination_count": sum(len(samples) for samples in score_map.values()),
                        "min_possible_score": min(score_map),
                        "max_possible_score": max(score_map),
                        "allowed_judgments": display_judgments(note_type, allowed[note_type]),
                    }
                )

    states: dict[int, list[dict[str, dict[str, int]]]] = {0: [{}]}
    for note_type, score_map in per_type_scores:
        next_states: dict[int, list[dict[str, dict[str, int]]]] = {}
        for previous_score, previous_samples in states.items():
            for score, count_samples in score_map.items():
                new_score = previous_score + score
                if new_score > max_score:
                    continue
                score_samples = next_states.setdefault(new_score, [])
                for previous_counts in previous_samples:
                    for counts in count_samples:
                        new_counts = dict(previous_counts)
                        if counts:
                            new_counts[note_type] = counts
                        if not append_limited(score_samples, new_counts, max_solutions):
                            truncated = True
                if count_state_entries(next_states) > max_states:
                    raise MaimaiError("combined search exceeded max_states")
        states = next_states

    candidate_scores = sorted(score for score in states if min_score <= score <= max_score)
    solutions = []
    matching_combination_count = 0
    for score in candidate_scores:
        matching_combination_count += len(states[score])
        for state_counts in states[score]:
            if len(solutions) >= max_solutions:
                truncated = True
                continue
            raw_counts = {nt: state_counts.get(nt, {}) for nt in NOTE_TYPES}
            totals = score_counts({"counts": raw_counts, "score_mode": score_mode})["totals"]
            out_counts = {nt: compact_counts(raw_counts[nt], nt) for nt in NOTE_TYPES}
            solutions.append({"score": score, "counts": out_counts, "totals": totals})

    result = {
        "found": bool(candidate_scores),
        "score_mode": score_mode,
        "target_range": {"min_score": min_score, "max_score": max_score},
        "matching_score_count": len(candidate_scores),
        "matching_combination_count": matching_combination_count,
        "matching_combination_count_is_exact": not truncated,
        "returned_solution_count": len(solutions),
        "truncated": truncated,
        "solutions": solutions,
        "per_type_summary": per_type_summary,
        "judgment_groups": judgment_group_reference(),
    }
    if shortcut_constraints:
        result["shortcut_constraints"] = shortcut_constraints
    return result


def dxacc_metric_config(note_totals: dict[str, int]) -> tuple[int, int, dict[str, dict[str, int]], dict[str, Any]]:
    max_base = max_base_score(note_totals)
    if max_base <= 0:
        raise MaimaiError("dxacc requires at least one scoring note")
    max_break_bonus = note_totals["break"] * 100
    denominator = math.lcm(max_base, max_break_bonus) if max_break_bonus else max_base
    base_weight = 100 * denominator // max_base
    break_bonus_weight = denominator // max_break_bonus if max_break_bonus else 0
    max_metric = max_base * base_weight + max_break_bonus * break_bonus_weight
    loss_values = {
        note_type: dxacc_loss_values(note_type, base_weight, break_bonus_weight)
        for note_type in NOTE_TYPES
    }
    details = {
        "max_base": max_base,
        "max_break_bonus": max_break_bonus,
        "denominator": denominator,
        "max_metric": max_metric,
        "base_weight": base_weight,
        "break_bonus_weight": break_bonus_weight,
    }
    return denominator, max_metric, loss_values, details


def dxacc_loss_values(
    note_type: str,
    base_weight: int,
    break_bonus_weight: int,
) -> dict[str, int]:
    max_base_value = max(BASE_VALUES[note_type].values())
    max_bonus_value = 100 if note_type == "break" else 0
    max_metric_value = max_base_value * base_weight + max_bonus_value * break_bonus_weight
    values = {}
    for judgment in JUDGMENTS:
        earned = BASE_VALUES[note_type][judgment] * base_weight
        if note_type == "break":
            earned += BREAK_BONUS_VALUES[judgment] * break_bonus_weight
        values[judgment] = max_metric_value - earned
    return values


def oldacc_metric_config(note_totals: dict[str, int]) -> tuple[int, int, dict[str, dict[str, int]], dict[str, Any]]:
    max_base = max_base_score(note_totals)
    if max_base <= 0:
        raise MaimaiError("oldacc requires at least one scoring note")
    max_old = max_old_score(note_totals)
    max_metric = max_old * 100
    loss_values: dict[str, dict[str, int]] = {}
    for note_type in NOTE_TYPES:
        max_value = max(OLD_SCORE_VALUES[note_type].values()) * 100
        loss_values[note_type] = {
            judgment: max_value - OLD_SCORE_VALUES[note_type][judgment] * 100
            for judgment in JUDGMENTS
        }
    details = {
        "max_base": max_base,
        "max_old": max_old,
        "denominator": max_base,
        "max_metric": max_metric,
    }
    return max_base, max_metric, loss_values, details


def acc_loss_range(
    arguments: dict[str, Any],
    max_metric: int,
    denominator: int,
    display_digits: int,
    display_mode: str,
    score_mode: str,
) -> tuple[int, int, dict[str, str]]:
    target = target_acc_fraction(arguments, display_digits)
    unit = Fraction(1, 10**display_digits)
    min_bound = acc_bound_fraction(
        arguments,
        "min_score",
        ("min_acc", "min_percent", "min_percentage"),
        display_digits,
    )
    max_bound = acc_bound_fraction(
        arguments,
        "max_score",
        ("max_acc", "max_percent", "max_percentage"),
        display_digits,
    )

    if target is not None:
        if min_bound is not None or max_bound is not None:
            raise MaimaiError("use either target_score/target_acc or min_score/max_score, not both")
        if display_mode == "floor":
            lower = target
            upper = target + unit
            min_loss = floor_fraction(Fraction(max_metric, 1) - upper * denominator) + 1
            max_loss = floor_fraction(Fraction(max_metric, 1) - lower * denominator)
            mode_detail = f"raw {score_mode} in [target, target + display unit)"
        elif display_mode == "half_up":
            lower = target - unit / 2
            upper = target + unit / 2
            min_loss = floor_fraction(Fraction(max_metric, 1) - upper * denominator) + 1
            max_loss = floor_fraction(Fraction(max_metric, 1) - lower * denominator)
            mode_detail = f"raw {score_mode} rounds half-up to target"
        elif display_mode == "exact":
            target_metric = target * denominator
            if target_metric.denominator != 1:
                return 1, 0, {
                    "min": format_fraction_decimal(target),
                    "max": format_fraction_decimal(target),
                    "mode_detail": "exact target is not representable for this chart denominator",
                }
            loss = max_metric - target_metric.numerator
            min_loss = loss
            max_loss = loss
            lower = target
            upper = target
            mode_detail = f"raw {score_mode} exactly equals target"
        else:
            raise MaimaiError(f"display_mode must be one of: {', '.join(ACHIEVEMENT_DISPLAY_MODES)}")
    else:
        if min_bound is None and max_bound is None:
            raise MaimaiError("target_score, target_acc, or min_score/max_score is required")
        lower = min_bound if min_bound is not None else Fraction(0, 1)
        upper_inclusive = max_bound if max_bound is not None else Fraction(10**9, 1)
        if lower > upper_inclusive:
            raise MaimaiError("minimum percentage cannot be greater than maximum percentage")
        min_loss = ceil_fraction(Fraction(max_metric, 1) - upper_inclusive * denominator)
        max_loss = floor_fraction(Fraction(max_metric, 1) - lower * denominator)
        upper = upper_inclusive
        mode_detail = f"raw {score_mode} in inclusive min/max range"

    return max(0, min_loss), max_loss, {
        "min": format_fraction_decimal(lower),
        "max_exclusive": format_fraction_decimal(upper) if target is not None and display_mode != "exact" else None,
        "max": format_fraction_decimal(upper) if target is None or display_mode == "exact" else None,
        "mode_detail": mode_detail,
    }


def find_acc_combinations(arguments: dict[str, Any], score_mode: str) -> dict[str, Any]:
    note_totals = normalize_note_totals(arguments.get("note_totals"))
    display_digits = parse_non_negative_int_like(arguments.get("display_digits", 4), "display_digits")
    if display_digits <= 0 or display_digits > 8:
        raise MaimaiError("display_digits must be between 1 and 8")
    display_mode = clean_name(str(arguments.get("display_mode", "floor")))
    if display_mode not in ACHIEVEMENT_DISPLAY_MODES:
        raise MaimaiError(f"display_mode must be one of: {', '.join(ACHIEVEMENT_DISPLAY_MODES)}")

    if score_mode == "dxacc":
        denominator, max_metric, loss_values_by_type, metric_details = dxacc_metric_config(note_totals)
    elif score_mode == "oldacc":
        denominator, max_metric, loss_values_by_type, metric_details = oldacc_metric_config(note_totals)
    else:
        raise MaimaiError("score_mode must be dxacc or oldacc")

    min_loss, max_loss, acc_range = acc_loss_range(
        arguments,
        max_metric,
        denominator,
        display_digits,
        display_mode,
        score_mode,
    )

    max_solutions = parse_non_negative_int_like(arguments.get("max_solutions", 10), "max_solutions")
    max_states = parse_non_negative_int_like(arguments.get("max_states", 200000), "max_states")
    if max_solutions == 0:
        max_solutions = 1
    if max_states == 0:
        raise MaimaiError("max_states must be greater than zero")

    if max_loss < min_loss:
        metric = dict(metric_details)
        metric["loss_range"] = {"min": min_loss, "max": max_loss}
        return {
            "found": False,
            "score_mode": score_mode,
            "target_type": score_mode,
            "display_mode": display_mode,
            "display_digits": display_digits,
            "note_totals": note_totals,
            "target_range": acc_range,
            f"{score_mode}_range": acc_range,
            f"{score_mode}_metric": metric,
            "solutions": [],
            "judgment_groups": judgment_group_reference(),
        }

    allowed = normalize_restriction_map(arguments.get("allowed_judgments"), "allowed_judgments", True)
    disallowed = normalize_restriction_map(arguments.get("disallowed_judgments"), "disallowed_judgments", False)
    for note_type in NOTE_TYPES:
        allowed[note_type] -= disallowed[note_type]

    fixed_counts = normalize_nested_counts(arguments.get("fixed_counts"), "fixed_counts")
    min_counts = normalize_nested_counts(arguments.get("min_counts"), "min_counts")
    max_counts = normalize_nested_counts(arguments.get("max_counts"), "max_counts")
    shortcut_constraints = apply_judgment_shortcuts(arguments, note_totals, allowed, min_counts)

    def _acc_per_type(nt: str) -> tuple[str, dict[int, list[dict[str, int]]], bool, dict[str, Any] | None]:
        lv = loss_values_by_type[nt]
        plan = build_note_type_plan(nt, note_totals[nt], "oldscore", allowed[nt], fixed_counts[nt], min_counts[nt], max_counts[nt])
        fixed_loss = sum(lv[j] * count for j, count in plan.base_counts.items())
        plan = NoteTypePlan(plan.note_type, plan.judgments, plan.base_counts, plan.remaining, plan.max_remaining, fixed_loss)
        sm, tr = possible_scores_for_plan(plan, "oldscore", max_states, max_solutions, score_values=lv, score_cap=max_loss)
        summary = None
        if not note_totals[nt]:
            pass
        elif not sm:
            raise MaimaiError(f"{nt} has no possible {score_mode} losses under the constraints")
        else:
            summary = {
                "note_type": nt,
                "possible_loss_count": len(sm),
                "sample_combination_count": sum(len(s) for s in sm.values()),
                "min_possible_loss": min(sm),
                "max_possible_loss": max(sm),
                "allowed_judgments": display_judgments(nt, allowed[nt]),
            }
        return nt, sm, tr, summary

    per_type_scores: list[tuple[str, dict[int, list[dict[str, int]]]]] = []
    per_type_summary = []
    truncated = False
    with ThreadPoolExecutor(max_workers=5) as executor:
        futures = {executor.submit(_acc_per_type, nt): nt for nt in NOTE_TYPES}
        for future in as_completed(futures):
            nt, sm, tr, summary = future.result()
            truncated = truncated or tr
            per_type_scores.append((nt, sm))
            if summary:
                per_type_summary.append(summary)

    states: dict[int, list[dict[str, dict[str, int]]]] = {0: [{}]}
    for note_type, score_map in per_type_scores:
        next_states: dict[int, list[dict[str, dict[str, int]]]] = {}
        for previous_loss, previous_samples in states.items():
            for loss, count_samples in score_map.items():
                new_loss = previous_loss + loss
                if new_loss > max_loss:
                    continue
                loss_samples = next_states.setdefault(new_loss, [])
                for previous_counts in previous_samples:
                    for counts in count_samples:
                        new_counts = dict(previous_counts)
                        if counts:
                            new_counts[note_type] = counts
                        if not append_limited(loss_samples, new_counts, max_solutions):
                            truncated = True
                if count_state_entries(next_states) > max_states:
                    raise MaimaiError(f"combined {score_mode} search exceeded max_states; add tighter judgment restrictions")
        states = next_states

    candidate_losses = sorted(loss for loss in states if min_loss <= loss <= max_loss)
    solutions = []
    matching_combination_count = 0
    for loss in candidate_losses:
        matching_combination_count += len(states[loss])
        for state_counts in states[loss]:
            if len(solutions) >= max_solutions:
                truncated = True
                continue
            raw_counts = {nt: state_counts.get(nt, {}) for nt in NOTE_TYPES}
            totals = score_counts({"counts": raw_counts, "score_mode": score_mode, "display_digits": display_digits})["totals"]
            acc = Fraction(max_metric - loss, denominator)
            out_counts = {nt: compact_counts(raw_counts[nt], nt) for nt in NOTE_TYPES}
            solutions.append(
                {
                    score_mode: percent_details_from_fraction(acc, display_digits),
                    f"{score_mode}_metric": max_metric - loss,
                    "loss_metric": loss,
                    "counts": out_counts,
                    "totals": totals,
                }
            )

    metric = dict(metric_details)
    metric["loss_range"] = {"min": min_loss, "max": max_loss}
    result = {
        "found": bool(candidate_losses),
        "score_mode": score_mode,
        "target_type": score_mode,
        "display_mode": display_mode,
        "display_digits": display_digits,
        "note_totals": note_totals,
        "target_range": acc_range,
        f"{score_mode}_range": acc_range,
        f"{score_mode}_metric": metric,
        f"matching_{score_mode}_count": len(candidate_losses),
        "matching_combination_count": matching_combination_count,
        "matching_combination_count_is_exact": not truncated,
        "returned_solution_count": len(solutions),
        "truncated": truncated,
        "solutions": solutions,
        "per_type_summary": per_type_summary,
        "judgment_groups": judgment_group_reference(),
    }
    if shortcut_constraints:
        result["shortcut_constraints"] = shortcut_constraints
    return result


TOOLS = [
    {
        "name": "score_counts",
        "description": (
            "Calculate maimai score contributions from note-type/judgment counts. "
            "Returns DX base, DX Break bonus, old-frame/FiNALE raw score, "
            "DX SCORE, old-frame percentage, and DX achievement percentage in one response."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "counts": {
                    "type": "object",
                    "description": "Nested counts: note type -> judgment -> count.",
                    "additionalProperties": {
                        "type": "object",
                        "additionalProperties": {"type": "integer", "minimum": 0},
                    },
                },
                "display_digits": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 8,
                    "default": 4,
                    "description": "Decimal digits used for oldacc/dxacc display and scaled values.",
                },
                "include_zero": {
                    "type": "boolean",
                    "default": False,
                    "description": "Include zero-count rows in the result table.",
                },
            },
        },
    },
    {
        "name": "find_score_combinations",
        "description": (
            "Search whether a chart can achieve an exact score or score range under judgment restrictions. "
            "Returns multiple matching judgment-count combinations up to max_solutions. "
            "Supports allowed/disallowed judgment groups and fixed/min/max counts per note type. "
            "score_mode=oldscore searches old-frame/FiNALE raw score. "
            "score_mode=dxscore searches DX SCORE. "
            "score_mode=oldacc or dxacc searches displayed percentage units; target_score=1004999 means 100.4999 when display_digits=4, and percentage strings such as 100.4999% are also accepted."
        ),
        "inputSchema": {
            "type": "object",
            "required": ["note_totals"],
            "properties": {
                "note_totals": {
                    "type": "object",
                    "description": "Chart note totals, e.g. {\"tap\": 100, \"touch\": 12, \"hold\": 20, \"slide\": 30, \"break\": 5}. Touch has the same value as Tap; touch_hold aliases are normalized to hold.",
                    "additionalProperties": {"type": "integer", "minimum": 0},
                },
                "target_score": {
                    "oneOf": [{"type": "integer", "minimum": 0}, {"type": "number", "minimum": 0}, {"type": "string"}],
                    "description": "Exact selected-mode score. For oldscore/dxscore/base/break_bonus use a raw integer. For oldacc/dxacc, an integer is scaled percentage without the decimal point, e.g. 1004999 means 100.4999 when display_digits=4; strings like 100.4999 or 100.4999% are accepted.",
                },
                "target_acc": {
                    "oneOf": [{"type": "number"}, {"type": "string"}],
                    "description": "Optional oldacc/dxacc target percentage such as 100.4999 or 100.4999%. Equivalent to target_score as a percentage string.",
                },
                "min_score": {
                    "oneOf": [{"type": "integer", "minimum": 0}, {"type": "number", "minimum": 0}, {"type": "string"}],
                    "description": "Minimum selected-mode score. In oldacc/dxacc, follows the same scaled-integer or percentage-string rules as target_score.",
                },
                "max_score": {
                    "oneOf": [{"type": "integer", "minimum": 0}, {"type": "number", "minimum": 0}, {"type": "string"}],
                    "description": "Maximum selected-mode score. In oldacc/dxacc, follows the same scaled-integer or percentage-string rules as target_score.",
                },
                "min_acc": {
                    "oneOf": [{"type": "number"}, {"type": "string"}],
                    "description": "Optional oldacc/dxacc minimum percentage such as 100.4000%.",
                },
                "max_acc": {
                    "oneOf": [{"type": "number"}, {"type": "string"}],
                    "description": "Optional oldacc/dxacc maximum percentage such as 100.5000%.",
                },
                "score_mode": {
                    "type": "string",
                    "enum": list(FIND_SCORE_MODES),
                    "default": "oldscore",
                    "description": (
                        "Search in base, break_bonus, oldscore raw score, oldacc percentage, "
                        "dxscore, or dxacc percentage. For oldacc/dxacc, target_score can be "
                        "a scaled display integer or a percentage string."
                    ),
                },
                "allowed_judgments": {
                    "type": "object",
                    "description": "Optional note type -> allowed judgments/groups. Groups include all, not_miss, no_miss, great, perfect, perfect_or_critical, ap, fc_plus, fc.",
                    "additionalProperties": {
                        "oneOf": [
                            {"type": "string"},
                            {"type": "array", "items": {"type": "string"}},
                        ]
                    },
                },
                "disallowed_judgments": {
                    "type": "object",
                    "description": "Optional note type -> forbidden judgments/groups, applied after allowed_judgments.",
                    "additionalProperties": {
                        "oneOf": [
                            {"type": "string"},
                            {"type": "array", "items": {"type": "string"}},
                        ]
                    },
                },
                "fixed_counts": {
                    "type": "object",
                    "description": "Exact counts to force: note type -> judgment -> count.",
                    "additionalProperties": {
                        "type": "object",
                        "additionalProperties": {"type": "integer", "minimum": 0},
                    },
                },
                "min_counts": {
                    "type": "object",
                    "description": "Minimum counts to force: note type -> judgment -> count.",
                    "additionalProperties": {
                        "type": "object",
                        "additionalProperties": {"type": "integer", "minimum": 0},
                    },
                },
                "max_counts": {
                    "type": "object",
                    "description": "Maximum counts to enforce: note type -> judgment -> count.",
                    "additionalProperties": {
                        "type": "object",
                        "additionalProperties": {"type": "integer", "minimum": 0},
                    },
                },
                "no_miss_good": {
                    "type": "boolean",
                    "description": "Shortcut restriction: all note types disallow miss and good.",
                },
                "all_notes_no_miss_good": {
                    "type": "boolean",
                    "description": "Alias of no_miss_good.",
                },
                "fc_plus_only": {
                    "type": "boolean",
                    "description": "Alias of no_miss_good; allows only great/perfect/critical judgments.",
                },
                "break_max_perfect_or_below": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Shortcut restriction: Break notes with judgment below Critical may be at most this many.",
                },
                "break_max_non_critical": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Alias of break_max_perfect_or_below.",
                },
                "max_break_non_critical": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Alias of break_max_perfect_or_below.",
                },
                "max_solutions": {
                    "type": "integer",
                    "minimum": 1,
                    "default": 10,
                    "description": "Maximum matching combinations to return. If more combinations exist, the response sets truncated=true.",
                },
                "max_states": {
                    "type": "integer",
                    "minimum": 1,
                    "default": 200000,
                    "description": "Search-state cap to avoid runaway combinatorics.",
                },
                "display_mode": {
                    "type": "string",
                    "enum": list(ACHIEVEMENT_DISPLAY_MODES),
                    "default": "floor",
                    "description": "Only for score_mode=oldacc or dxacc. Interprets target_score as floor display, half-up display, or exact raw percentage.",
                },
                "display_digits": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 8,
                    "default": 4,
                    "description": "Only for score_mode=oldacc or dxacc. Number of displayed decimal digits; default 4.",
                },
            },
        },
    },
]


def handle_tool_call(name: str, arguments: Any) -> dict[str, Any]:
    if arguments is None:
        arguments = {}
    if not isinstance(arguments, dict):
        raise MaimaiError("tool arguments must be an object")
    if name == "score_counts":
        return score_counts(arguments)
    if name == "find_score_combinations":
        return find_score_combinations(arguments)
    raise MaimaiError(f"unknown tool: {name}")


def response(message_id: Any, result: Any) -> dict[str, Any]:
    return {"jsonrpc": "2.0", "id": message_id, "result": result}


def error_response(message_id: Any, code: int, message: str) -> dict[str, Any]:
    return {"jsonrpc": "2.0", "id": message_id, "error": {"code": code, "message": message}}


def dispatch(message: dict[str, Any]) -> dict[str, Any] | None:
    method = message.get("method")
    message_id = message.get("id")

    if method == "initialize":
        params = message.get("params") or {}
        protocol_version = params.get("protocolVersion", "2024-11-05")
        return response(
            message_id,
            {
                "protocolVersion": protocol_version,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "maimai-dx-scoring", "version": "0.1.0"},
            },
        )
    if method == "notifications/initialized":
        return None
    if method == "tools/list":
        return response(message_id, {"tools": TOOLS})
    if method == "tools/call":
        params = message.get("params") or {}
        try:
            result = handle_tool_call(params.get("name"), params.get("arguments"))
            return response(
                message_id,
                {
                    "content": [
                        {
                            "type": "text",
                            "text": json.dumps(result, ensure_ascii=False, indent=2),
                        }
                    ],
                    "isError": False,
                },
            )
        except MaimaiError as exc:
            return response(
                message_id,
                {
                    "content": [{"type": "text", "text": str(exc)}],
                    "isError": True,
                },
            )

    if message_id is None:
        return None
    return error_response(message_id, -32601, f"method not found: {method}")


def run_stdio() -> int:
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            message = json.loads(line)
            if not isinstance(message, dict):
                raise ValueError("message must be an object")
            outgoing = dispatch(message)
        except json.JSONDecodeError as exc:
            outgoing = error_response(None, -32700, f"parse error: {exc}")
        except Exception as exc:  # Keep the MCP loop alive on unexpected failures.
            outgoing = error_response(None, -32603, f"internal error: {exc}")
        if outgoing is not None:
            print(json.dumps(outgoing, ensure_ascii=False), flush=True)
    return 0


def build_arg_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="maimai DX scoring MCP server")
    parser.add_argument(
        "--stdio",
        action="store_true",
        help="Run as an MCP stdio server. This is the default.",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    build_arg_parser().parse_args(argv)
    return run_stdio()


if __name__ == "__main__":
    raise SystemExit(main())
