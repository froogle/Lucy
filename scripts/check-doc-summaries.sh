#!/usr/bin/env bash
# A rustdoc summary, the first sentence of a /// or //! block, has at most
# 15 words (CODING_STANDARDS.md: Rustdoc documents entry points only).
set -euo pipefail
root=${1:-.}
most=15

find "$root/crates" -name '*.rs' -print0 | xargs -0 perl -ne '
    BEGIN { $most = shift @ARGV }
    if (m{^\s*(//[/!])(?![/!])\s?(.*)$}) {
        my ($marker, $text) = ($1, $2);
        if (!defined $block || $marker ne $block) {
            ($block, $start, $summary, $done) = ($marker, $., "", 0);
        }
        next if $done;
        if ($text =~ /^\s*$/ || $text =~ /^\s*(```|#\s)/) { $done = 1; next }
        $summary .= " $text";
        (my $plain = $summary) =~ s/`[^`]*`/CODE/g;
        if ($plain =~ /^(.*?[.?!])(\s|$)/) {
            $done = 1;
            my $words = () = $1 =~ /\S+/g;
            if ($words > $most) {
                print STDERR "$ARGV:$start: $words words: $summary\n";
                $failed = 1;
            }
        }
    } else {
        undef $block;
    }
    close ARGV if eof;
    END {
        if ($failed) {
            print STDERR "Cut each summary to at most $most words; detail goes in the paragraphs after it (CODING_STANDARDS.md).\n";
            exit 1;
        }
    }
' "$most"
