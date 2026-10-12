#!/bin/sh
# Records real programs into harness/corpus/real-<name>.bin. Re-runnable, but
# NOT part of tools/check.sh: the .bin files are recorded once and committed.
# macOS (BSD `script`). Programs that are not installed are skipped silently.
# Output is made self-contained: fixed fixture dir, fixed git repo (dates,
# author, committer), TERM/LC_ALL pinned, pty 80x24; machine/user-specific
# strings are scrubbed after recording.
set -u
here="$(cd "$(dirname "$0")" && pwd)"
fixture="$here/fixture"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

export TERM=xterm-256color LC_ALL=en_US.UTF-8 LANG=en_US.UTF-8
export GIT_AUTHOR_NAME=Fixture GIT_AUTHOR_EMAIL=fixture@example.invalid
export GIT_COMMITTER_NAME=Fixture GIT_COMMITTER_EMAIL=fixture@example.invalid
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null
export TZ=UTC CLICOLOR_FORCE=1 CLICOLOR=1
export LESSHISTFILE=- LESSSECURE=1

# Deterministic fixture dir: fixed content, mtimes and modes, owner scrubbed below.
mkdir -p "$fixture"
printf 'alpha\nbeta\ngamma\ndelta\nepsilon\n' > "$fixture/a.txt"
printf '#!/bin/sh\necho hello\n' > "$fixture/run.sh"
printf 'fn main() {}\n' > "$fixture/main.rs"
mkdir -p "$fixture/sub"
chmod 644 "$fixture/a.txt" "$fixture/main.rs"; chmod 755 "$fixture/run.sh" "$fixture/sub"
xattr -c "$fixture" "$fixture"/* 2>/dev/null
touch -t 202001020304 "$fixture/a.txt" "$fixture/run.sh" "$fixture/main.rs" "$fixture/sub"

# Tiny fixed repo with fixed dates.
repo="$work/repo"
mkdir -p "$repo"
(
  cd "$repo" && git init -q -b main .
  i=1
  while [ $i -le 5 ]; do
    echo "change $i" > "f$i.txt"; git add .
    GIT_AUTHOR_DATE="2020-01-0$i 12:00:00 +0000" GIT_COMMITTER_DATE="2020-01-0$i 12:00:00 +0000" \
      git -c commit.gpgsign=false commit -q -m "commit number $i"
    i=$((i + 1))
  done
  git tag v1.0 HEAD~2
)

# record <name> <script-body>: runs the body under a fresh 80x24 pty.
record() {
  name="$1"; body="$2"; out="$here/real-$name.bin"
  printf '%s\n' "$body" > "$work/$name.sh"
  # script(1) allocates the pty; stty inside sets the size before the program runs.
  script -q /dev/null /bin/sh -c "stty cols 80 rows 24; . '$work/$name.sh'" > "$work/$name.raw" < "${3:-/dev/null}"
  # Scrub: user, home, host, repo path, abbreviated hashes, top's volatile numbers.
  user="$(id -un)"; host="$(hostname -s)"
  sed -e "s#$HOME#/home/user#g" -e "s#$work#/tmp/work#g" \
      -e "s#$user#user#g" -e "s#$host#host#g" "$work/$name.raw" > "$out"
  size=$(wc -c < "$out" | tr -d ' ')
  if [ "$size" -gt 65536 ]; then echo "refusing real-$name.bin: $size bytes > 64 KB" >&2; rm -f "$out"; return 1; fi
  echo "recorded real-$name.bin ($size bytes)"
}

have() { command -v "$1" >/dev/null 2>&1; }

record ls-color "cd '$fixture' && ls -lAG"
have git && record git-log "cd '$work/repo' && git --no-pager log --oneline --decorate --color=always -n 30"

if have vim; then
  # Keystrokes arrive on stdin (a here-doc); the -c commands do the editing.
  printf ':q!\r' > "$work/vim.keys"
  record vim "cd '$fixture' && vim -u NONE -N -i NONE -n -c 'set nu' a.txt -c 'normal 3Gdd' -c ':q!'" "$work/vim.keys"
fi

# top: ANSI-light; header only (-n 0, process names are machine-specific) and
# volatile numbers are scrubbed to zeros.
if have top; then
  record top "top -l 1 -n 0 -s 0 | sed -E 's/[0-9]+/0/g'"
fi

have bat && record bat "cd '$fixture' && bat --color=always --style=numbers --paging=never a.txt main.rs"
if have less; then
  printf 'q' > "$work/less.keys"
  record less "cd '$fixture' && less -R a.txt" "$work/less.keys"
fi
exit 0
