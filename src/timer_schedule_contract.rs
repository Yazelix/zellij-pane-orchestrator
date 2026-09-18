use std::time::{Duration, Instant};

const MINIMUM_TIMER_DELAY: Duration = Duration::from_millis(500);

pub fn next_timer_delay<I>(
    now: Instant,
    deadlines: I,
    armed_deadline: Option<Instant>,
) -> Option<(Instant, Duration)>
where
    I: IntoIterator<Item = Option<Instant>>,
{
    let next_deadline = deadlines.into_iter().flatten().min()?;
    let delay = next_deadline.saturating_duration_since(now);
    let delay = delay.max(MINIMUM_TIMER_DELAY);
    let wake_at = now + delay;
    if armed_deadline.is_some_and(|deadline| now < deadline && deadline <= wake_at) {
        return None;
    }
    Some((wake_at, delay))
}

// Test lane: default
#[cfg(test)]
mod tests {
    use super::{next_timer_delay, MINIMUM_TIMER_DELAY};
    use std::time::{Duration, Instant};

    // Regression: multiple orchestrator refresh loops must share one Zellij timeout instead of arming a timeout per non-due loop.
    #[test]
    fn arms_only_the_earliest_unarmed_deadline() {
        let now = Instant::now();
        let early = now + Duration::from_secs(2);
        let later = now + Duration::from_secs(120);

        assert_eq!(
            next_timer_delay(now, [Some(later), None, Some(early)], None),
            Some((early, Duration::from_secs(2)))
        );
        assert_eq!(
            next_timer_delay(now, [Some(later), None, Some(early)], Some(early)),
            None
        );
        assert_eq!(next_timer_delay(now, [Some(later)], Some(early)), None);
    }

    // Regression: a stale Zellij timer callback must not leave an expired deadline
    // blocking the next timer after a shorter timeout preempted it.
    #[test]
    fn replaces_an_expired_armed_deadline() {
        let now = Instant::now();
        let next = now + Duration::from_secs(2);

        assert_eq!(
            next_timer_delay(now, [Some(next)], Some(now - Duration::from_secs(1))),
            Some((next, Duration::from_secs(2)))
        );
    }

    // Defends: overdue work is handled promptly without asking Zellij for sub-frame timer churn.
    #[test]
    fn clamps_overdue_deadlines_to_minimum_delay() {
        let now = Instant::now();
        let overdue = now - Duration::from_secs(3);
        let wake_at = now + MINIMUM_TIMER_DELAY;

        assert_eq!(
            next_timer_delay(now, [Some(overdue)], None),
            Some((wake_at, MINIMUM_TIMER_DELAY))
        );
        assert_eq!(next_timer_delay(now, [Some(overdue)], Some(wake_at)), None);
    }
}
