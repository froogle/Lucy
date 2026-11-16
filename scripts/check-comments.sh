#!/usr/bin/env bash
# A plain // comment is a defect (CODING_STANDARDS.md: code explains itself).
# Allowed: rustdoc (/// and //!), // SAFETY: before unsafe, and the
# // sleep-ok: that scripts/check-test-sleeps.sh asks for, each with the
# comment-only lines straight after it at the same indentation.
set -euo pipefail
root=${1:-.}

find "$root/crates" -name '*.rs' -print0 | xargs -0 perl -0777 -ne '
    my $src = $_;
    while ($src =~ m{
        ( b?r(\#*)".*?"\2 )                  # raw string
      | ( b?"(?:\\.|[^"\\])*" )              # string
      | ( b?\x27(?:\\.|[^\\\x27\n])\x27 )    # char
      | ( //[^\n]* )                         # line comment
      | ( /\*.*?\*/ )                        # block comment
    }gsx) {
        my $comment = $5 // $6;
        next unless defined $comment;
        next if $comment =~ m{^//[/!](?!/)};
        my $before = substr($src, 0, $-[0]);
        my $line = 1 + ($before =~ tr/\n//);
        my ($lead) = $before =~ m{([^\n]*)\z};
        my $indent = $lead =~ m{\A[ \t]*\z} ? length($lead) : -1;
        my $continues = $indent >= 0
            && ($allowed_through{$ARGV} // -1) == $line - 1
            && $allowed_indent{$ARGV} == $indent;
        if ($comment =~ m{^// (SAFETY|sleep-ok):} || $continues) {
            $allowed_through{$ARGV} = $line;
            $allowed_indent{$ARGV} = $indent unless $continues;
            next;
        }
        print STDERR "$ARGV:$line: $comment\n";
        $failed = 1;
    }
    END {
        if ($failed) {
            print STDERR "Say it in the code instead: rename, extract a function, add a type or name a constant (CODING_STANDARDS.md).\n";
            print STDERR "  Rationale goes in an ADR under docs/adr/; only rustdoc, // SAFETY: and // sleep-ok: comments stay.\n";
            exit 1;
        }
    }
'
