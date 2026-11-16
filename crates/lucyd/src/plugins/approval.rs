use super::manifest::{Manifest, ToolEntry};

pub fn summary(manifest: &Manifest) -> String {
    let handles: Vec<&str> = manifest.tools.iter().map(|t| &*t.handles).collect();
    let mut said = vec![format!(
        "The {} Plugin needs your Approval. It handles {}.",
        manifest.name,
        and(&handles)
    )];
    let access: Vec<String> = kinds(manifest)
        .into_iter()
        .filter(|(_, items)| !items.is_empty())
        .map(|(verb, items)| format!("{verb} {}", and(items)))
        .collect();
    said.push(if access.is_empty() {
        "It needs no access to your files, the network or other programs.".to_owned()
    } else {
        format!("It can {}.", and(&access))
    });
    let asks: Vec<&str> = manifest
        .tools
        .iter()
        .filter(|t| t.confirm)
        .map(|t| &*t.handles)
        .collect();
    said.push(if asks.is_empty() {
        "It never asks you before acting.".to_owned()
    } else {
        format!("It asks you before it will {}.", or(&asks))
    });
    said.push("Do you approve it?".to_owned());
    said.join(" ")
}

pub fn changes(old: &Manifest, new: &Manifest) -> String {
    let mut said = vec![format!(
        "The {} Plugin has changed and needs your Approval again.",
        new.name
    )];
    for tool in &new.tools {
        match find(old, &tool.name) {
            None => said.push(format!("It now handles {}.", tool.handles)),
            Some(before) => {
                if before.handles != tool.handles {
                    said.push(format!(
                        "It now handles {} instead of {}.",
                        tool.handles, before.handles
                    ));
                }
                if tool.confirm && !before.confirm {
                    said.push(format!("It now asks you before it will {}.", tool.handles));
                }
                if !tool.confirm && before.confirm {
                    said.push(format!(
                        "It no longer asks you before it will {}.",
                        tool.handles
                    ));
                }
            }
        }
    }
    for tool in &old.tools {
        if find(new, &tool.name).is_none() {
            said.push(format!("It no longer handles {}.", tool.handles));
        }
    }
    for ((verb, before), (_, after)) in kinds(old).into_iter().zip(kinds(new)) {
        let added: Vec<&str> = after
            .iter()
            .filter(|item| !before.contains(item))
            .map(String::as_str)
            .collect();
        let removed: Vec<&str> = before
            .iter()
            .filter(|item| !after.contains(item))
            .map(String::as_str)
            .collect();
        if !added.is_empty() {
            said.push(format!("It can now {verb} {}.", and(&added)));
        }
        if !removed.is_empty() {
            said.push(format!("It can no longer {verb} {}.", and(&removed)));
        }
    }
    if said.len() == 1 {
        said.push(
            "What it handles, can access and asks about is the same; its code or wording changed."
                .to_owned(),
        );
    }
    said.push("The full diff is on screen. Do you approve the update?".to_owned());
    said.join(" ")
}

fn find<'a>(manifest: &'a Manifest, name: &str) -> Option<&'a ToolEntry> {
    manifest.tools.iter().find(|t| t.name == name)
}

fn kinds(manifest: &Manifest) -> [(&'static str, &[String]); 6] {
    let access = &manifest.access;
    [
        ("read", &access.read),
        ("write to", &access.write),
        ("reach", &access.network),
        ("use the credentials", &access.credentials),
        ("run", &access.commands),
        ("see the environment variables", &manifest.requires.env),
    ]
}

pub fn diff(old: &str, new: &str) -> String {
    if old == new {
        return "The Manifest is the same; only the executable changed.".to_owned();
    }
    let (old, new): (Vec<&str>, Vec<&str>) = (old.lines().collect(), new.lines().collect());
    let mut lcs_of_tails = vec![vec![0usize; new.len() + 1]; old.len() + 1];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            lcs_of_tails[i][j] = if old[i] == new[j] {
                lcs_of_tails[i + 1][j + 1] + 1
            } else {
                lcs_of_tails[i + 1][j].max(lcs_of_tails[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut lines = Vec::new();
    while i < old.len() || j < new.len() {
        if i < old.len() && j < new.len() && old[i] == new[j] {
            lines.push(format!("  {}", old[i]));
            (i, j) = (i + 1, j + 1);
        } else if i < old.len()
            && (j == new.len() || lcs_of_tails[i + 1][j] >= lcs_of_tails[i][j + 1])
        {
            lines.push(format!("- {}", old[i]));
            i += 1;
        } else {
            lines.push(format!("+ {}", new[j]));
            j += 1;
        }
    }
    lines.join("\n")
}

fn and<S: AsRef<str>>(items: &[S]) -> String {
    list(items, "and")
}

fn or<S: AsRef<str>>(items: &[S]) -> String {
    list(items, "or")
}

fn list<S: AsRef<str>>(items: &[S], word: &str) -> String {
    match items {
        [] => String::new(),
        [only] => only.as_ref().to_owned(),
        [rest @ .., last] => {
            let rest: Vec<&str> = rest.iter().map(AsRef::as_ref).collect();
            format!("{} {word} {}", rest.join(", "), last.as_ref())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_diff_marks_removed_and_added_lines_and_keeps_the_rest() {
        assert_eq!(diff("a\nb\nc", "a\nB\nc\nd"), "  a\n- b\n+ B\n  c\n+ d");
    }
}
