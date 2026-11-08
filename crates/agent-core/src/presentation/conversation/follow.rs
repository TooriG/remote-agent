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
/// or by following, follows again; anything else keeps `following`.
pub fn follows_end(following: bool, previous: Option<ScrollPosition>, now: ScrollPosition) -> bool {
    let toward_start = previous.is_some_and(|previous| {
        now.offset < previous.offset - 0.5 && now.content >= previous.content - 0.5
    });
    if toward_start && now.content > now.viewport {
        return false;
    }
    if now.content - now.offset - now.viewport <= FOLLOW_REARM_DISTANCE {
        return true;
    }
    following
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
            if gap <= FOLLOW_REARM_DISTANCE {
                proptest::prop_assert!(follows_end(following, None, now));
            } else {
                proptest::prop_assert_eq!(follows_end(following, None, now), following);
            }
            proptest::prop_assert!(!follows_end(following, Some(below), now));
        }
    }
}
