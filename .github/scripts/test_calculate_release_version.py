"""Unit tests for calculate_release_version.py.

Run with: python -m pytest .github/scripts/test_calculate_release_version.py -v
"""
from __future__ import annotations

import pytest

from calculate_release_version import (
    bump_base,
    calculate_version,
    detect_current_level,
    latest_stable_from_tags,
    next_dry_run_counter,
    next_prerelease_counter,
    parse_version,
    to_pep440,
)


# ---------- parse_version ----------


class TestParseVersion:
    def test_stable(self):
        v = parse_version("1.2.3")
        assert (v.major, v.minor, v.patch) == (1, 2, 3)
        assert v.prerelease_label is None
        assert v.prerelease_num is None

    def test_prerelease(self):
        v = parse_version("0.12.0-next.5")
        assert (v.major, v.minor, v.patch) == (0, 12, 0)
        assert v.prerelease_label == "next"
        assert v.prerelease_num == 5

    @pytest.mark.parametrize("label", ["alpha", "beta", "rc", "next"])
    def test_all_prerelease_labels(self, label):
        v = parse_version(f"1.2.3-{label}.7")
        assert v.prerelease_label == label
        assert v.prerelease_num == 7

    def test_invalid(self):
        with pytest.raises(ValueError):
            parse_version("not-a-version")


# ---------- bump_base ----------


class TestBumpBase:
    @pytest.mark.parametrize(
        "base,bump,expected",
        [
            ("0.11.7", "patch", "0.11.8"),
            ("0.11.7", "minor", "0.12.0"),
            ("0.11.7", "major", "1.0.0"),
            ("1.2.3", "minor", "1.3.0"),
            ("0.0.0", "patch", "0.0.1"),
        ],
    )
    def test_bump(self, base, bump, expected):
        assert bump_base(base, bump) == expected

    def test_unknown_bump(self):
        with pytest.raises(ValueError):
            bump_base("1.0.0", "bogus")


# ---------- detect_current_level ----------


class TestDetectCurrentLevel:
    @pytest.mark.parametrize(
        "current,stable,expected",
        [
            ("0.11.7", "0.11.6", "patch"),
            ("0.12.0", "0.11.6", "minor"),
            ("1.0.0", "0.11.6", "major"),
            ("0.11.6", "0.11.6", None),  # equal — no bump implied
            ("0.10.0", "0.11.6", None),  # current older
        ],
    )
    def test_levels(self, current, stable, expected):
        assert detect_current_level(current, stable) == expected

    def test_no_stable(self):
        assert detect_current_level("0.1.0", None) is None


# ---------- next_prerelease_counter ----------


class TestPrereleaseCounter:
    def test_empty(self):
        assert next_prerelease_counter("0.12.0", "next", [], "iii") == 1

    def test_existing(self):
        tags = ["iii/v0.12.0-next.1", "iii/v0.12.0-next.2", "iii/v0.12.0-next.3"]
        assert next_prerelease_counter("0.12.0", "next", tags, "iii") == 4

    def test_different_base_ignored(self):
        tags = ["iii/v0.11.0-next.1", "iii/v0.13.0-next.5"]
        assert next_prerelease_counter("0.12.0", "next", tags, "iii") == 1

    def test_different_label_ignored(self):
        tags = ["iii/v0.12.0-rc.1", "iii/v0.12.0-alpha.3"]
        assert next_prerelease_counter("0.12.0", "next", tags, "iii") == 1

    def test_different_prefix_ignored(self):
        tags = ["other/v0.12.0-next.5"]
        assert next_prerelease_counter("0.12.0", "next", tags, "iii") == 1

    def test_picks_max_not_count(self):
        tags = ["iii/v0.12.0-next.1", "iii/v0.12.0-next.7"]
        assert next_prerelease_counter("0.12.0", "next", tags, "iii") == 8


# ---------- next_dry_run_counter ----------


class TestDryRunCounter:
    def test_empty(self):
        assert next_dry_run_counter("0.12.0", [], "iii") == 1

    def test_existing(self):
        tags = ["iii/v0.12.0-dry-run.1", "iii/v0.12.0-dry-run.4"]
        assert next_dry_run_counter("0.12.0", tags, "iii") == 5


# ---------- latest_stable_from_tags ----------


class TestLatestStable:
    def test_picks_highest(self):
        tags = [
            "iii/v0.11.5",
            "iii/v0.11.6",
            "iii/v0.11.6-next.3",  # prerelease ignored
            "iii/v0.10.0",
        ]
        assert latest_stable_from_tags(tags, "iii") == "0.11.6"

    def test_no_stable(self):
        tags = ["iii/v0.12.0-next.1", "iii/v0.12.0-next.2"]
        assert latest_stable_from_tags(tags, "iii") is None

    def test_empty(self):
        assert latest_stable_from_tags([], "iii") is None

    def test_different_prefix_ignored(self):
        tags = ["other/v9.9.9", "iii/v0.1.0"]
        assert latest_stable_from_tags(tags, "iii") == "0.1.0"


# ---------- calculate_version ----------


class TestCalculateVersion:
    """All the version-bump scenarios.

    The naming convention is: starting_state__bump_prerelease__expected.
    """

    # -- Stable starting point --

    def test_stable_patch_stable(self):
        assert calculate_version("0.11.6", "patch", "none", "0.11.6", [], "iii") == "0.11.7"

    def test_stable_minor_stable(self):
        assert calculate_version("0.11.6", "minor", "none", "0.11.6", [], "iii") == "0.12.0"

    def test_stable_major_stable(self):
        assert calculate_version("0.11.6", "major", "none", "0.11.6", [], "iii") == "1.0.0"

    def test_stable_patch_next(self):
        assert calculate_version("0.11.6", "patch", "next", "0.11.6", [], "iii") == "0.11.7-next.1"

    def test_stable_minor_next(self):
        assert calculate_version("0.11.6", "minor", "next", "0.11.6", [], "iii") == "0.12.0-next.1"

    def test_stable_major_rc(self):
        assert calculate_version("0.11.6", "major", "rc", "0.11.6", [], "iii") == "1.0.0-rc.1"

    # -- Patch always advances the patch component --

    def test_patch_always_bumps_patch_component(self):
        """patch is never an iteration: it always advances the patch component."""
        assert (
            calculate_version("0.11.7-next.2", "patch", "next", "0.11.6", [], "iii")
            == "0.11.8-next.1"
        )

    def test_patch_from_minor_prerelease_bumps_patch_component(self):
        """Even mid-minor-train, patch advances to a new patch base."""
        tags = ["iii/v0.12.0-next.1", "iii/v0.12.0-next.2"]
        assert (
            calculate_version("0.12.0-next.2", "patch", "next", "0.11.6", tags, "iii")
            == "0.12.1-next.1"
        )

    def test_patch_stable_to_next(self):
        assert (
            calculate_version("0.11.6", "patch", "next", "0.11.6", [], "iii")
            == "0.11.7-next.1"
        )

    # -- Patch prerelease train + non-patch bumps --

    def test_patch_prerelease_minor_next_escalates(self):
        """The bug from the original report: minor must escalate, not continue counter."""
        assert (
            calculate_version("0.11.7-next.2", "minor", "next", "0.11.6", [], "iii")
            == "0.12.0-next.1"
        )

    def test_patch_prerelease_major_next_escalates(self):
        assert (
            calculate_version("0.11.7-next.2", "major", "next", "0.11.6", [], "iii")
            == "1.0.0-next.1"
        )

    # -- Minor prerelease train (the second bug the user reported) --

    def test_minor_prerelease_minor_next_continues_train(self):
        """After escalating to 0.12.0-next.1, subsequent minor+next must iterate, not bump again."""
        tags = ["iii/v0.12.0-next.1"]
        assert (
            calculate_version("0.12.0-next.1", "minor", "next", "0.11.6", tags, "iii")
            == "0.12.0-next.2"
        )

    def test_minor_prerelease_major_next_escalates(self):
        assert (
            calculate_version("0.12.0-next.1", "major", "next", "0.11.6", [], "iii")
            == "1.0.0-next.1"
        )

    # -- Major prerelease train --

    def test_major_prerelease_major_next_continues_train(self):
        tags = ["iii/v1.0.0-next.1", "iii/v1.0.0-next.2"]
        assert (
            calculate_version("1.0.0-next.2", "major", "next", "0.11.6", tags, "iii")
            == "1.0.0-next.3"
        )

    def test_major_prerelease_minor_next_continues_train(self):
        tags = ["iii/v1.0.0-next.1"]
        assert (
            calculate_version("1.0.0-next.1", "minor", "next", "0.11.6", tags, "iii")
            == "1.0.0-next.2"
        )

    # -- Promotion to stable --

    def test_promote_prerelease_to_stable_drops_suffix(self):
        assert calculate_version("0.12.0-next.5", "patch", "none", "0.11.6", [], "iii") == "0.12.0"

    def test_promote_with_minor_bump_still_uses_base(self):
        """Once a prerelease train decided the base, promotion uses it as-is."""
        assert calculate_version("0.12.0-next.5", "minor", "none", "0.11.6", [], "iii") == "0.12.0"

    # -- Channel switching --

    def test_next_to_rc_starts_fresh_counter(self):
        tags = ["iii/v0.12.0-next.3"]
        assert (
            calculate_version("0.12.0-next.3", "minor", "rc", "0.11.6", tags, "iii")
            == "0.13.0-rc.1"
        )

    # -- No prior stable tag (bootstrap) --

    def test_no_stable_minor_bumps_from_current(self):
        """Without a stable anchor, minor must still apply to the current base."""
        tags = ["iii/v0.1.0-next.1"]
        assert (
            calculate_version("0.1.0-next.1", "minor", "next", None, tags, "iii")
            == "0.2.0-next.1"
        )

    def test_no_stable_stable_starts_from_current(self):
        assert calculate_version("0.0.1", "minor", "next", None, [], "iii") == "0.1.0-next.1"

    def test_no_stable_patch_bumps_from_current(self):
        tags = ["iii/v0.1.0-next.1"]
        assert (
            calculate_version("0.1.0-next.1", "patch", "next", None, tags, "iii")
            == "0.1.1-next.1"
        )

    # -- Counter resumes from existing max --

    def test_counter_uses_max_existing(self):
        tags = [
            "iii/v0.12.0-next.1",
            "iii/v0.12.0-next.7",
            "iii/v0.12.0-next.3",
        ]
        assert (
            calculate_version("0.12.0-next.7", "minor", "next", "0.11.6", tags, "iii")
            == "0.12.0-next.8"
        )

    # -- Invalid inputs --

    def test_unknown_bump(self):
        with pytest.raises(ValueError):
            calculate_version("0.1.0", "bogus", "none", "0.1.0", [], "iii")

    def test_unknown_prerelease(self):
        with pytest.raises(ValueError):
            calculate_version("0.1.0", "patch", "weird", "0.1.0", [], "iii")


# ---------- to_pep440 ----------


class TestAdversarial:
    """Edge cases designed to break the algorithm."""

    # -- Counter-bump ordering --

    def test_counter_uses_numeric_max_not_lex(self):
        """Lex sort would put next.10 before next.2; integer comparison must win."""
        tags = [
            "iii/v0.12.0-next.1",
            "iii/v0.12.0-next.2",
            "iii/v0.12.0-next.10",
        ]
        assert (
            calculate_version("0.12.0-next.10", "minor", "next", "0.11.6", tags, "iii")
            == "0.12.0-next.11"
        )

    def test_counter_picks_max_with_huge_gaps(self):
        tags = ["iii/v0.12.0-next.1", "iii/v0.12.0-next.999"]
        assert (
            calculate_version("0.12.0-next.999", "minor", "next", "0.11.6", tags, "iii")
            == "0.12.0-next.1000"
        )

    # -- Tag prefix isolation --

    def test_other_prefix_tags_do_not_pollute_counter(self):
        tags = [
            "console/v0.12.0-next.50",
            "other/v0.12.0-next.99",
            "iii/v0.12.0-next.1",
        ]
        assert (
            calculate_version("0.12.0-next.1", "minor", "next", "0.11.6", tags, "iii")
            == "0.12.0-next.2"
        )

    def test_substring_prefix_does_not_match(self):
        """`iii` must not match `iiii/...` or `iii-extra/...`."""
        tags = ["iiii/v0.12.0-next.50", "iii-extra/v0.12.0-next.30"]
        assert (
            calculate_version("0.12.0-next.1", "minor", "next", "0.11.6", tags, "iii")
            == "0.12.0-next.1"
        )

    def test_latest_stable_ignores_substring_prefix(self):
        tags = ["iiii/v9.9.9", "iii-extra/v8.8.8", "iii/v0.5.0"]
        assert latest_stable_from_tags(tags, "iii") == "0.5.0"

    def test_tag_prefix_with_regex_special_chars(self):
        """Prefix containing `.` must be literal, not regex wildcard."""
        tags = ["myXtarget/v0.12.0-next.50", "my.target/v0.12.0-next.1"]
        assert (
            calculate_version("0.12.0-next.1", "minor", "next", "0.11.6", tags, "my.target")
            == "0.12.0-next.2"
        )

    # -- Cross-channel counter isolation --

    def test_other_channels_do_not_affect_next_counter(self):
        tags = [
            "iii/v0.12.0-rc.5",
            "iii/v0.12.0-alpha.7",
            "iii/v0.12.0-beta.3",
            "iii/v0.12.0-next.1",
        ]
        assert (
            calculate_version("0.12.0-next.1", "minor", "next", "0.11.6", tags, "iii")
            == "0.12.0-next.2"
        )

    def test_switching_channel_starts_fresh(self):
        tags = ["iii/v0.12.0-next.5"]
        assert (
            calculate_version("0.12.0-next.5", "minor", "rc", "0.11.6", tags, "iii")
            == "0.13.0-rc.1"
        )

    def test_patch_on_rc_train_advances_patch_component(self):
        tags = ["iii/v0.12.0-rc.1", "iii/v0.12.0-rc.2"]
        assert (
            calculate_version("0.12.0-rc.2", "patch", "rc", "0.11.6", tags, "iii")
            == "0.12.1-rc.1"
        )

    # -- Promotion semantics --

    def test_promote_with_major_bump_still_keeps_base(self):
        """Once a prerelease decided the base, promotion uses it regardless of bump type."""
        assert (
            calculate_version("0.12.0-next.5", "major", "none", "0.11.6", [], "iii")
            == "0.12.0"
        )

    def test_promote_from_rc_to_stable(self):
        assert (
            calculate_version("0.12.0-rc.7", "patch", "none", "0.11.6", [], "iii")
            == "0.12.0"
        )

    def test_stable_to_stable_does_not_trigger_promotion(self):
        """Promotion branch is gated on current having a prerelease label."""
        assert (
            calculate_version("0.12.0", "patch", "none", "0.12.0", [], "iii")
            == "0.12.1"
        )

    # -- Latest_stable equal to or ahead of current --

    def test_current_equals_stable_no_level_detected(self):
        """If current and stable are equal, the prerelease is degenerate; bump anyway."""
        tags = ["iii/v0.12.0", "iii/v0.12.0-next.1"]
        # detect_current_level returns None (cur == stable), so we apply bump to current base.
        assert (
            calculate_version("0.12.0-next.1", "minor", "next", "0.12.0", tags, "iii")
            == "0.13.0-next.1"
        )

    def test_current_older_than_stable_treated_as_no_level(self):
        """Pathological: cargo says 0.10.0-next.1 but stable has moved to 0.11.6."""
        assert (
            calculate_version("0.10.0-next.1", "minor", "next", "0.11.6", [], "iii")
            == "0.11.0-next.1"
        )

    # -- Zero-component edge cases --

    def test_zero_base_patch(self):
        assert calculate_version("0.0.0", "patch", "next", None, [], "iii") == "0.0.1-next.1"

    def test_zero_base_major(self):
        assert calculate_version("0.0.0", "major", "next", None, [], "iii") == "1.0.0-next.1"

    def test_large_numbers(self):
        assert (
            calculate_version("999.999.999", "patch", "next", "999.999.998", [], "iii")
            == "999.999.1000-next.1"
        )

    # -- Sequential walk: each result feeds the next call --

    def test_full_release_sequence(self):
        """The whole release cycle the user described, in order."""
        tags = ["iii/v0.11.6"]
        stable = "0.11.6"

        def step(current, bump, pre):
            result = calculate_version(current, bump, pre, stable, tags, "iii")
            tags.append(f"iii/v{result}")
            return result

        assert step("0.11.6", "patch", "next") == "0.11.7-next.1"
        assert step("0.11.7-next.1", "patch", "next") == "0.11.8-next.1"  # patch always advances
        assert step("0.11.8-next.1", "minor", "next") == "0.12.0-next.1"  # escalate
        assert step("0.12.0-next.1", "minor", "next") == "0.12.0-next.2"  # iterate
        assert step("0.12.0-next.2", "minor", "next") == "0.12.0-next.3"  # iterate
        assert step("0.12.0-next.3", "patch", "none") == "0.12.0"          # promote

    def test_rc_iteration_sequence(self):
        tags = ["iii/v0.11.6"]
        stable = "0.11.6"

        def step(current, bump, pre):
            result = calculate_version(current, bump, pre, stable, tags, "iii")
            tags.append(f"iii/v{result}")
            return result

        assert step("0.11.6", "minor", "rc") == "0.12.0-rc.1"
        assert step("0.12.0-rc.1", "minor", "rc") == "0.12.0-rc.2"
        assert step("0.12.0-rc.2", "major", "rc") == "1.0.0-rc.1"
        assert step("1.0.0-rc.1", "major", "rc") == "1.0.0-rc.2"
        assert step("1.0.0-rc.2", "patch", "none") == "1.0.0"

    # -- Malformed tag handling in counter scan --

    def test_malformed_tags_ignored(self):
        tags = [
            "iii/v0.12.0-next.1",
            "iii/not-a-version",
            "iii/v0.12.0-next.abc",
            "garbage",
            "iii/v0.12.0-next.",
        ]
        assert (
            calculate_version("0.12.0-next.1", "minor", "next", "0.11.6", tags, "iii")
            == "0.12.0-next.2"
        )

    # -- Mixed major+minor latest_stable behavior --

    def test_minor_jump_after_major_release(self):
        """After releasing 1.0.0, subsequent minor+next prereleases."""
        tags = ["iii/v1.0.0"]
        assert (
            calculate_version("1.0.0", "minor", "next", "1.0.0", tags, "iii")
            == "1.1.0-next.1"
        )
        tags.append("iii/v1.1.0-next.1")
        assert (
            calculate_version("1.1.0-next.1", "minor", "next", "1.0.0", tags, "iii")
            == "1.1.0-next.2"
        )

    # -- Counter starts past 1 if existing tags found at new base --

    def test_existing_tags_at_target_base_continue_counter(self):
        """If we escalate to a base that already has prereleases, resume the counter."""
        tags = [
            "iii/v0.11.6",
            "iii/v0.12.0-next.1",  # someone else already started this train
            "iii/v0.12.0-next.2",
        ]
        assert (
            calculate_version("0.11.7-next.5", "minor", "next", "0.11.6", tags, "iii")
            == "0.12.0-next.3"
        )

    # -- Patch + same channel with existing patch-base tags --

    def test_patch_resumes_counter_at_new_patch_base(self):
        """patch advances the patch component, but if tags already exist at that base, resume."""
        tags = [
            "iii/v0.11.6",
            "iii/v0.11.7-next.1",
        ]
        assert (
            calculate_version("0.11.6", "patch", "next", "0.11.6", tags, "iii")
            == "0.11.7-next.2"
        )


class TestToPep440:
    @pytest.mark.parametrize(
        "version,expected",
        [
            ("0.12.0", "0.12.0"),
            ("0.12.0-rc.1", "0.12.0rc1"),
            ("0.12.0-alpha.3", "0.12.0a3"),
            ("0.12.0-beta.2", "0.12.0b2"),
            ("0.12.0-next.5", "0.12.0.dev5"),
        ],
    )
    def test_conversion(self, version, expected):
        assert to_pep440(version) == expected

    def test_unknown_label_passes_through(self):
        # Dry-run tags aren't valid PEP 440 prereleases; we just leave them.
        assert to_pep440("0.12.0-dry-run.1") == "0.12.0-dry-run.1"

    def test_unknown_label_matching_regex_passes_through(self):
        # `dev` is [a-z]+ so it matches PRERELEASE_RE but isn't in the channel
        # mapping. Must passthrough untouched rather than producing garbage.
        assert to_pep440("1.2.3-dev.4") == "1.2.3-dev.4"
        assert to_pep440("1.2.3-foo.4") == "1.2.3-foo.4"


# ---------- Adversarial (post-merge hardening) ----------
#
# These tests document behavior we discovered while attacking the algorithm
# after the initial port. They lock in current behavior — including a few
# documented footguns — so future refactors don't silently drift.


class TestPromotionFootguns:
    """Promotion to `none` always returns the current base, regardless of bump.

    This is intentional ("drop the prerelease suffix, keep the base") but is a
    sharp edge: a user on `0.15.0-next.5` who runs `patch + none` expecting
    `0.15.1` will get `0.15.0`. Documented here so the surprise is visible.
    """

    def test_promote_ignores_patch_bump(self):
        assert (
            calculate_version("0.15.0-next.5", "patch", "none", "0.14.0", ["iii/v0.14.0"], "iii")
            == "0.15.0"
        )

    def test_promote_ignores_minor_bump(self):
        assert (
            calculate_version("0.15.0-next.5", "minor", "none", "0.14.0", ["iii/v0.14.0"], "iii")
            == "0.15.0"
        )

    def test_promote_ignores_major_bump(self):
        assert (
            calculate_version("0.15.0-next.5", "major", "none", "0.14.0", ["iii/v0.14.0"], "iii")
            == "0.15.0"
        )

    def test_stable_to_stable_does_not_promote(self):
        # No prerelease label on current → promotion branch skipped → real bump.
        assert (
            calculate_version("0.15.0", "patch", "none", "0.15.0", ["iii/v0.15.0"], "iii")
            == "0.15.1"
        )


class TestCurrentBehindLatestStable:
    """If engine/Cargo.toml falls behind the latest stable tag (manual
    downgrade, bad merge), the algorithm anchors from the *current* base
    rather than `latest_stable`. That can produce a tag that overlaps an
    earlier train. Locked in here so we notice if the behavior changes.
    """

    def test_minor_below_stable_anchors_from_current(self):
        assert (
            calculate_version(
                "0.12.0-next.5", "minor", "next", "0.15.0", ["iii/v0.15.0"], "iii"
            )
            == "0.13.0-next.1"
        )

    def test_major_jump_when_current_minor_below_stable_major(self):
        # Current's major < stable's major → detect_current_level returns None
        # → falls into the "no level" branch → bumps the current base.
        assert (
            calculate_version(
                "0.5.0-next.1", "minor", "next", "1.2.0", ["iii/v1.2.0"], "iii"
            )
            == "0.6.0-next.1"
        )

    def test_current_equals_stable_no_level_minor(self):
        # cur == stable → detect_current_level None → bump current base.
        assert (
            calculate_version("0.13.0", "minor", "next", "0.13.0", ["iii/v0.13.0"], "iii")
            == "0.14.0-next.1"
        )


class TestChannelSwitchPicksUpExistingCounter:
    """Switching channels (e.g. rc → next) uses the next_prerelease_counter
    scan on the target base, which resumes from any existing tags. This is
    intentional (avoids clobbering published prereleases) but can surprise
    someone who expects `.1` after a fresh switch.
    """

    def test_switch_resumes_counter_from_existing_target_tag(self):
        tags = ["iii/v0.16.0-next.3", "iii/v0.13.0"]
        # rc.2 → minor + next bumps base to 0.16.0; existing 0.16.0-next.3
        # forces counter to 4 rather than 1.
        assert (
            calculate_version("0.15.0-rc.2", "minor", "next", "0.13.0", tags, "iii")
            == "0.16.0-next.4"
        )

    def test_switch_starts_at_1_when_no_existing_on_target_base(self):
        tags = ["iii/v0.13.0", "iii/v0.15.0-rc.2"]
        assert (
            calculate_version("0.15.0-rc.2", "minor", "next", "0.13.0", tags, "iii")
            == "0.16.0-next.1"
        )


class TestSameChannelStaysIsolatedFromOtherChannels:
    """A `next` release must not be polluted by `rc` (or alpha/beta) tags."""

    def test_next_ignores_rc_tags_at_same_base(self):
        tags = [
            "iii/v0.15.0-rc.5",
            "iii/v0.15.0-rc.6",
            "iii/v0.15.0-rc.7",
            "iii/v0.15.0-next.1",
        ]
        assert (
            calculate_version("0.15.0-next.1", "minor", "next", "0.13.0", tags, "iii")
            == "0.15.0-next.2"
        )

    def test_rc_ignores_next_tags_at_same_base(self):
        tags = [
            "iii/v0.15.0-next.5",
            "iii/v0.15.0-next.6",
            "iii/v0.15.0-rc.1",
        ]
        assert (
            calculate_version("0.15.0-rc.1", "minor", "rc", "0.13.0", tags, "iii")
            == "0.15.0-rc.2"
        )


class TestCounterAtZero:
    """A `.next.0` tag in the wild shouldn't break counter math."""

    def test_zero_counter_increments(self):
        tags = ["iii/v0.15.0-next.0"]
        assert (
            calculate_version("0.15.0-next.0", "minor", "next", "0.13.0", tags, "iii")
            == "0.15.0-next.1"
        )


class TestNoLatestStable:
    """First-ever release: no stable tag yet, current is the seed prerelease."""

    def test_same_channel_minor_bumps_current_when_no_stable(self):
        assert (
            calculate_version("0.5.0-next.5", "minor", "next", None, ["iii/v0.5.0-next.5"], "iii")
            == "0.6.0-next.1"
        )

    def test_same_channel_patch_advances_patch_when_no_stable(self):
        assert (
            calculate_version("0.5.0-next.5", "patch", "next", None, [], "iii")
            == "0.5.1-next.1"
        )


class TestEscalationRestartsFromStable:
    """When the requested bump escalates beyond the current train's level,
    we restart from the latest stable rather than the current base.
    """

    def test_major_from_minor_train_anchors_at_stable(self):
        # current 0.20.0-next.1, stable 0.13.0 → current_level=minor, request=major
        # → escalate → anchor=latest_stable=0.13.0 → bump major → 1.0.0
        assert (
            calculate_version(
                "0.20.0-next.1", "major", "next", "0.13.0", ["iii/v0.13.0"], "iii"
            )
            == "1.0.0-next.1"
        )

    def test_minor_from_patch_train_anchors_at_stable(self):
        # current 0.13.5-next.1 (patch train above stable 0.13.0), request minor
        # → escalate → anchor=0.13.0 → bump minor → 0.14.0
        assert (
            calculate_version(
                "0.13.5-next.1", "minor", "next", "0.13.0", ["iii/v0.13.0"], "iii"
            )
            == "0.14.0-next.1"
        )


class TestCargoVersionParsing:
    """`_read_cargo_version` returns the FIRST line that starts with
    `version = `. A Cargo.toml whose first such line is in a dependency
    table (rather than `[package]`) would yield the wrong version. Locked
    in so we catch any regression in the parser.
    """

    def test_picks_first_line_starting_with_version(self, tmp_path):
        from calculate_release_version import _read_cargo_version

        p = tmp_path / "Cargo.toml"
        p.write_text('[package]\nname = "iii"\nversion = "1.2.3"\n')
        assert _read_cargo_version(str(p)) == "1.2.3"

    def test_indented_version_lines_are_skipped(self, tmp_path):
        # `startswith("version = ")` requires column-0, so indented version
        # lines inside dependency tables are correctly ignored.
        from calculate_release_version import _read_cargo_version

        p = tmp_path / "Cargo.toml"
        p.write_text(
            '[dependencies.foo]\n    version = "9.9.9"\n[package]\nversion = "1.2.3"\n'
        )
        assert _read_cargo_version(str(p)) == "1.2.3"

    def test_missing_version_raises(self, tmp_path):
        from calculate_release_version import _read_cargo_version

        p = tmp_path / "Cargo.toml"
        p.write_text("[package]\nname = \"iii\"\n")
        with pytest.raises(RuntimeError):
            _read_cargo_version(str(p))


class TestCalculateRejectsBadInputs:
    def test_unknown_bump_type_raises(self):
        with pytest.raises(ValueError):
            calculate_version("0.13.0", "bogus", "next", "0.13.0", [], "iii")

    def test_unknown_prerelease_raises(self):
        with pytest.raises(ValueError):
            calculate_version("0.13.0", "minor", "bogus", "0.13.0", [], "iii")

    def test_empty_prerelease_string_raises(self):
        with pytest.raises(ValueError):
            calculate_version("0.13.0", "minor", "", "0.13.0", [], "iii")

    def test_uppercase_prerelease_raises(self):
        # Only lowercase channel labels are accepted; CLI should normalize
        # upstream if it ever supports anything else.
        with pytest.raises(ValueError):
            calculate_version("0.13.0", "minor", "NEXT", "0.13.0", [], "iii")

    def test_unparseable_current_raises(self):
        with pytest.raises(ValueError):
            calculate_version("not-a-version", "minor", "next", "0.13.0", [], "iii")


class TestDryRunInteraction:
    """`--dry-run` is applied after `calculate_version`. It splits on the
    first `-` of whatever calculate produced and tacks on a dry-run counter.
    Verify the boundary cases.
    """

    def test_dry_run_counter_on_stable_base(self):
        from calculate_release_version import next_dry_run_counter

        tags = ["iii/v0.16.0-dry-run.1", "iii/v0.16.0-dry-run.5"]
        assert next_dry_run_counter("0.16.0", tags, "iii") == 6

    def test_dry_run_counter_starts_at_1_when_none(self):
        from calculate_release_version import next_dry_run_counter

        assert next_dry_run_counter("0.16.0", [], "iii") == 1

    def test_dry_run_ignores_other_prefixes(self):
        from calculate_release_version import next_dry_run_counter

        tags = ["other/v0.16.0-dry-run.99", "iii/v0.16.0-dry-run.1"]
        assert next_dry_run_counter("0.16.0", tags, "iii") == 2


class TestLargeCounterMath:
    def test_counter_well_above_int32(self):
        # Python ints are unbounded; ensure huge counters work.
        tags = [f"iii/v0.15.0-next.{2**40}"]
        assert (
            calculate_version("0.15.0-next.1", "minor", "next", "0.13.0", tags, "iii")
            == f"0.15.0-next.{2**40 + 1}"
        )


class TestTagPatternRejectsImpostors:
    """Bogus or malformed entries in the tag list must not affect counters."""

    def test_partial_match_does_not_count(self):
        tags = [
            "iii/v0.15.0-next.1abc",  # trailing garbage — must be rejected
            "iii/v0.15.0-next.",  # missing number
            "iii/v0.15.0-next",  # missing dot+number
            "iii/v0.15.0-NEXT.5",  # uppercase channel
            "iii/v0.15.0-next.1",  # only this one should count
        ]
        assert (
            calculate_version("0.15.0-next.1", "minor", "next", "0.13.0", tags, "iii")
            == "0.15.0-next.2"
        )

    def test_blank_lines_in_tag_list(self):
        # `_git_tags` filters blanks but `next_prerelease_counter` should be
        # robust regardless of upstream filtering.
        tags = ["", "iii/v0.15.0-next.3", ""]
        assert (
            calculate_version("0.15.0-next.1", "minor", "next", "0.13.0", tags, "iii")
            == "0.15.0-next.4"
        )
