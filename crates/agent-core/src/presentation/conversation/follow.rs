//! Whether a conversation follows its end as a turn streams in, as T3 Code's
//! timeline does (`liveFollowEnabled`, `maintainScrollAtEnd`): the reader
//! scrolling toward the start, however little, by wheel, trackpad or drag,
//! stops following until they come back to the end.

/// How near the end the reader counts as at it, and follows again (T3 Code's
/// `TIMELINE_FOLLOW_REARM_THRESHOLD_PX`).
pub const FOLLOW_REARM_DISTANCE: f32 = 40.;

/// Where a scrolled conversation is, in points along its axis.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "bindings", derive(uniffi::Record))]
pub struct ScrollPosition {
    /// How far the content is scrolled from its start.
    pub offset: f32,
    pub viewport: f32,
    pub content: f32,
}

/// Whether the conversation follows its end once it moved from `previous` to
/// `now`. Moving toward the start stops following, unless the content shrank
/// under the reader or cannot scroll at all; reaching the end, by the reader
/// or by following, follows again; anything else keeps `following`. Without
/// a `previous` position, a following conversation was at its end.
pub fn follows_end(following: bool, previous: Option<ScrollPosition>, now: ScrollPosition) -> bool {
    let previous = previous.or(following.then_some(ScrollPosition {
        offset: (now.content - now.viewport).max(0.),
        ..now
    }));
    let toward_start = previous.is_some_and(|previous| {
        now.offset < previous.offset - 0.5 && now.content >= previous.content - 0.5
    });
    if toward_start && now.content > now.viewport {
        return false;
    }
    if !away_from_end(now) {
        return true;
    }
    following
}

/// Whether the reader is further from the end than the follow band, where
/// T3 Code offers its scroll-to-bottom button.
pub fn away_from_end(position: ScrollPosition) -> bool {
    position.content - position.offset - position.viewport > FOLLOW_REARM_DISTANCE
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(offset: f32, content: f32) -> ScrollPosition {
        ScrollPosition {
            offset,
            viewport: 500.,
            content,
        }
    }

    #[test]
    fn a_small_scroll_up_while_a_turn_streams_stops_following_until_the_end() {
        // At the end, following.
        let end = at(1500., 2000.);
        assert!(follows_end(true, None, end));
        // Four lines up stay within the re-arm band, yet stop following.
        let up = at(1470., 2000.);
        assert!(!follows_end(true, Some(end), up));
        // The turn streams on: the content grows under the reader, who stays.
        let grown = at(1470., 2400.);
        assert!(!follows_end(false, Some(up), grown));
        // Scrolling down short of the end keeps not following.
        let down = at(1700., 2400.);
        assert!(!follows_end(false, Some(grown), down));
        // Back at the end, it follows again.
        let back = at(1870., 2400.);
        assert!(follows_end(false, Some(down), back));
    }

    #[test]
    fn following_scrolls_and_shrinking_content_do_not_stop_following() {
        // Following moves toward the end as the content grows.
        assert!(follows_end(true, Some(at(1500., 2000.)), at(1900., 2400.)));
        // A fold closing near the end pulls the offset back with the content.
        assert!(follows_end(true, Some(at(1900., 2400.)), at(1500., 2000.)));
        // Content shorter than the viewport cannot leave the end.
        assert!(follows_end(true, Some(at(10., 300.)), at(0., 300.)));
    }

    #[test]
    fn the_way_back_shows_only_beyond_the_follow_band() {
        assert!(!away_from_end(at(1500., 2000.)));
        assert!(!away_from_end(at(1460., 2000.)));
        assert!(away_from_end(at(1459., 2000.)));
        // Content that fits has no end to be away from.
        assert!(!away_from_end(at(0., 300.)));
    }

    #[test]
    fn the_first_scroll_of_a_following_conversation_leaves_its_end() {
        // Nothing scrolled the list since it opened at its end.
        assert!(!follows_end(true, None, at(1300., 2000.)));
        assert!(!follows_end(true, None, at(1490., 2000.)));
        assert!(follows_end(true, None, at(1500., 2000.)));
        // A reader who left the end keeps their place.
        assert!(!follows_end(false, None, at(1300., 2000.)));
    }

    proptest::proptest! {
        #[test]
        fn the_end_follows_and_an_upward_scroll_never_does(
            following in proptest::bool::ANY,
            offset in 0f32..5000.,
            delta in 1f32..2000.,
            content in 600f32..8000.,
        ) {
            let now = at(offset.min(content - 500.).max(0.), content);
            let gap = now.content - now.offset - now.viewport;
            let below = at(now.offset + delta, content);
            // A following conversation was at its end: any scroll above it,
            // even within the band, leaves it.
            let left = following && now.offset < now.content - now.viewport - 0.5;
            if gap <= FOLLOW_REARM_DISTANCE && !left {
                proptest::prop_assert!(follows_end(following, None, now));
            } else {
                // Away from the end, the reader left it.
                proptest::prop_assert!(!follows_end(following, None, now));
            }
            proptest::prop_assert!(!follows_end(following, Some(below), now));
        }
    }
}
