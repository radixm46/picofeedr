use crate::error::AppError;
use ::time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};
use time::macros::format_description;

const DATE_FORMAT: &[time::format_description::BorrowedFormatItem<'static>] =
    format_description!("[year]-[month]-[day]");

/// Parses an ISO date (YYYY-MM-DD) to epoch seconds at local midnight.
fn parse_date_to_epoch(value: &str, local_offset: UtcOffset) -> Result<i64, AppError> {
    let date =
        Date::parse(value, DATE_FORMAT).map_err(|_| AppError::invalid_query("Invalid date"))?;
    let datetime = PrimitiveDateTime::new(date, Time::MIDNIGHT);
    Ok(datetime.assume_offset(local_offset).unix_timestamp())
}

/// Parses either absolute date (`YYYY-MM-DD`) or relative duration (`N[d|w|m|y]`) to epoch seconds.
pub(super) fn parse_date_or_relative_to_epoch(
    value: &str,
    now_epoch_utc: i64,
    local_offset: UtcOffset,
) -> Result<i64, AppError> {
    if let Ok(epoch) = parse_date_to_epoch(value, local_offset) {
        return Ok(epoch);
    }
    parse_relative_date_to_epoch(value, now_epoch_utc, local_offset)
}

/// Parses relative date duration (`N[d|w|m|y]`) anchored at local-date midnight.
fn parse_relative_date_to_epoch(
    value: &str,
    now_epoch_utc: i64,
    local_offset: UtcOffset,
) -> Result<i64, AppError> {
    let (amount, unit) = parse_relative_duration(value)?;
    let now_utc = OffsetDateTime::from_unix_timestamp(now_epoch_utc)
        .map_err(|_| AppError::invalid_query("Invalid relative date"))?;
    let base_date = now_utc.to_offset(local_offset).date();
    let target_date = match unit {
        'd' => base_date
            .checked_sub(time::Duration::days(amount as i64))
            .ok_or_else(|| AppError::invalid_query("Invalid relative date"))?,
        'w' => base_date
            .checked_sub(time::Duration::days((amount as i64) * 7))
            .ok_or_else(|| AppError::invalid_query("Invalid relative date"))?,
        'm' => subtract_months_clamped(base_date, amount)?,
        'y' => subtract_years_clamped(base_date, amount)?,
        _ => unreachable!("parse_relative_duration restricts relative date units"),
    };
    Ok(PrimitiveDateTime::new(target_date, Time::MIDNIGHT)
        .assume_offset(local_offset)
        .unix_timestamp())
}

/// Parses `N[d|w|m|y]` into (amount, unit).
fn parse_relative_duration(value: &str) -> Result<(u32, char), AppError> {
    if value.len() < 2 || !value.is_ascii() {
        return Err(AppError::invalid_query("Invalid relative date"));
    }
    let (number, unit) = value.split_at(value.len() - 1);
    if number.is_empty() || number.starts_with('-') || !number.chars().all(|ch| ch.is_ascii_digit())
    {
        return Err(AppError::invalid_query("Invalid relative date"));
    }
    let amount = number
        .parse::<u32>()
        .map_err(|_| AppError::invalid_query("Invalid relative date"))?;
    let unit = unit
        .chars()
        .next()
        .ok_or_else(|| AppError::invalid_query("Invalid relative date"))?;
    if !matches!(unit, 'd' | 'w' | 'm' | 'y') {
        return Err(AppError::invalid_query("Invalid relative date"));
    }
    Ok((amount, unit))
}

/// Subtracts months while clamping day at month-end.
fn subtract_months_clamped(date: Date, months: u32) -> Result<Date, AppError> {
    let months_i32 =
        i32::try_from(months).map_err(|_| AppError::invalid_query("Invalid relative date"))?;
    let month_index = i32::from(u8::from(date.month())) - 1;
    let total = date
        .year()
        .checked_mul(12)
        .and_then(|value| value.checked_add(month_index))
        .and_then(|value| value.checked_sub(months_i32))
        .ok_or_else(|| AppError::invalid_query("Invalid relative date"))?;
    let year = total.div_euclid(12);
    let month = Month::try_from((total.rem_euclid(12) + 1) as u8)
        .map_err(|_| AppError::invalid_query("Invalid relative date"))?;
    let day = date.day().min(month.length(year));
    Date::from_calendar_date(year, month, day)
        .map_err(|_| AppError::invalid_query("Invalid relative date"))
}

/// Subtracts years while clamping day at month-end.
fn subtract_years_clamped(date: Date, years: u32) -> Result<Date, AppError> {
    let years_i32 =
        i32::try_from(years).map_err(|_| AppError::invalid_query("Invalid relative date"))?;
    let year = date
        .year()
        .checked_sub(years_i32)
        .ok_or_else(|| AppError::invalid_query("Invalid relative date"))?;
    let month = date.month();
    let day = date.day().min(month.length(year));
    Date::from_calendar_date(year, month, day)
        .map_err(|_| AppError::invalid_query("Invalid relative date"))
}

#[cfg(test)]
mod tests {
    use super::parse_date_or_relative_to_epoch;
    use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};

    fn fixed_now_utc() -> i64 {
        OffsetDateTime::new_utc(
            Date::from_calendar_date(2026, Month::February, 26).expect("date"),
            Time::from_hms(3, 0, 0).expect("time"),
        )
        .unix_timestamp()
    }

    fn fixed_jst() -> UtcOffset {
        UtcOffset::from_hms(9, 0, 0).expect("offset")
    }

    fn local_midnight_epoch(year: i32, month: Month, day: u8, offset: UtcOffset) -> i64 {
        PrimitiveDateTime::new(
            Date::from_calendar_date(year, month, day).expect("date"),
            Time::MIDNIGHT,
        )
        .assume_offset(offset)
        .unix_timestamp()
    }

    #[test]
    fn parse_absolute_date_bounds_use_local_midnight() {
        assert_eq!(
            parse_date_or_relative_to_epoch("2026-01-01", fixed_now_utc(), fixed_jst())
                .expect("epoch"),
            local_midnight_epoch(2026, Month::January, 1, fixed_jst())
        );
    }

    #[test]
    fn parse_relative_year_from_leap_day_clamps_to_feb_28() {
        let leap_day_now = OffsetDateTime::new_utc(
            Date::from_calendar_date(2024, Month::February, 29).expect("date"),
            Time::from_hms(3, 0, 0).expect("time"),
        )
        .unix_timestamp();
        assert_eq!(
            parse_date_or_relative_to_epoch("1y", leap_day_now, fixed_jst()).expect("epoch"),
            local_midnight_epoch(2023, Month::February, 28, fixed_jst())
        );
    }

    #[test]
    fn rejects_non_ascii_relative_date_suffixes() {
        for value in ["1好", "好", "1🍣"] {
            let error =
                parse_date_or_relative_to_epoch(value, fixed_now_utc(), fixed_jst()).unwrap_err();
            assert_eq!(error.code().as_str(), "INVALID_QUERY", "value: {value}");
        }
    }
}
