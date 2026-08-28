"""Non-standard extensions from Python: off unless asked for.

An extension is not a dialect setting. A dialect chooses between readings
some reference implementation answers with; an extension departs from all of
them, so nothing here is on by default and the flagged answers are pinned by
hand rather than against an oracle. docs/extensions.md is the prose.
"""

import subprocess
import sys

import pytest

import jay
from jay import JayError, apl, j
from jay.lang import APL, J


class TestJUnicodeStrings:
    """J's literal type is one byte per item; the flag makes it characters."""

    def test_a_literal_is_bytes_by_default(self):
        assert j("# 'é'") == 2
        assert j("# 'héllo'") == 6
        assert j("3 u: 'é'").tolist() == [195, 169]

    def test_the_flag_makes_it_characters(self):
        assert j("# 'é'", extensions="j_unicode_strings") == 1
        assert j("# 'héllo'", extensions="j_unicode_strings") == 5
        assert j("3 u: 'é'", extensions="j_unicode_strings") == 233

    def test_a_list_of_names_is_taken_too(self):
        assert j("# 'é'", extensions=["j_unicode_strings"]) == 1

    def test_a_compiler_can_carry_the_flag(self):
        compiler = J.create_compiler(extensions="j_unicode_strings")
        assert compiler.compile("# 'é'")() == 1
        assert j.compile("# 'é'")() == 2

    def test_the_display_is_the_text_either_way(self):
        assert j.compile("'héllo'").run_display() == "héllo"
        assert (
            j.compile("'héllo'", extensions="j_unicode_strings").run_display() == "héllo"
        )

    def test_apl_is_untouched(self):
        assert apl("≢'héllo'", extensions="j_unicode_strings") == 5
        assert APL.create_compiler(extensions="j_unicode_strings").compile("≢'héllo'")() == 5


class TestTheMechanism:
    def test_an_unknown_extension_is_refused_by_name(self):
        with pytest.raises(JayError) as e:
            j("1", extensions="j_unicode_string")
        assert "unknown extension" in str(e.value)
        assert "j_unicode_strings" in str(e.value)

    def test_naming_none_is_the_language_as_it_ships(self):
        assert j("# 'é'", extensions=()) == 2

    def test_the_set_is_part_of_the_cache_key(self):
        plain = j.compile("# 'é'")
        flagged = j.compile("# 'é'", extensions="j_unicode_strings")
        assert plain._inner is not flagged._inner
        assert j.compile("# 'é'")._inner is plain._inner


class TestTheCatalogue:
    def test_the_build_lists_what_it_has(self):
        names = {e.name for e in jay.extensions()}
        assert "j_unicode_strings" in names
        one = next(e for e in jay.extensions() if e.name == "j_unicode_strings")
        assert one.env == "LIBJAY_J_UNICODE_STRINGS"
        assert one.description


class TestTheCli:
    """`libjay --extension NAME` says what a run departs in."""

    def run(self, *args):
        return subprocess.run(
            [sys.executable, "-m", "jay._cli", *args],
            capture_output=True,
            text=True,
        )

    def test_the_default_is_the_language_as_it_ships(self):
        out = self.run("-e", "# 'é'")
        assert out.returncode == 0, out.stderr
        assert out.stdout.strip() == "2"

    def test_the_flag_switches_the_extension_on(self):
        out = self.run("-e", "# 'é'", "--extension", "j_unicode_strings")
        assert out.returncode == 0, out.stderr
        assert out.stdout.strip() == "1"

    def test_an_unknown_name_is_reported(self):
        out = self.run("-e", "1", "--extension", "nonesuch")
        assert out.returncode == 1
        assert "unknown extension" in out.stderr
