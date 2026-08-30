use git2::Repository;

pub(crate) fn github_remote_url(repo: &Repository, hosts: &[String]) -> Option<String> {
    let Ok(remotes) = repo.remotes() else {
        return None;
    };

    remotes.iter().flatten().find_map(|name| {
        repo.find_remote(name)
            .ok()
            .and_then(|remote| match remote.pushurl() {
                Some(url) => github_web_url_from_remote(url, hosts),
                None => remote
                    .url()
                    .and_then(|url| github_web_url_from_remote(url, hosts)),
            })
    })
}

pub(crate) fn has_origin_remote(repo: &Repository) -> bool {
    repo.find_remote("origin").is_ok()
}

#[cfg(test)]
pub(crate) fn default_github_hosts() -> Vec<String> {
    vec!["github.com".to_string()]
}

pub(crate) fn github_repo_name_from_web_url(url: &str) -> Option<&str> {
    url.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
}

fn github_web_url_from_remote(url: &str, hosts: &[String]) -> Option<String> {
    let trimmed = url.trim();
    let (host, path) = split_git_remote(trimmed)?;
    if !hosts.iter().any(|allowed| same_host(allowed, host)) {
        return None;
    }

    let path = path
        .split(['?', '#'])
        .next()
        .unwrap_or(path)
        .trim_end_matches('/')
        .trim_end_matches(".git");
    let mut parts = path.split('/');
    let owner = parts.next().filter(|part| is_safe_url_segment(part))?;
    let repo = parts.next().filter(|part| is_safe_url_segment(part))?;
    Some(format!("https://{host}/{owner}/{repo}"))
}

// GitHub owner/repo names only allow these; also keeps shell metachars out of
// the URL handed to the OS opener (`cmd /C start` on Windows).
fn is_safe_url_segment(part: &str) -> bool {
    !part.is_empty()
        && part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn split_git_remote(url: &str) -> Option<(&str, &str)> {
    for scheme in ["git+ssh://", "ssh://", "https://", "http://"] {
        if let Some(rest) = url.strip_prefix(scheme) {
            let rest = rest
                .rsplit_once('@')
                .map(|(_, value)| value)
                .unwrap_or(rest);
            let (authority, path) = rest.split_once('/')?;
            let host = authority
                .split_once(':')
                .map(|(host, _port)| host)
                .unwrap_or(authority);
            return Some((host, path));
        }
    }

    if let Some(rest) = url.strip_prefix("git@") {
        return rest.split_once(':');
    }

    url.split_once(':')
}

fn same_host(allowed: &str, actual: &str) -> bool {
    normalize_host(allowed).eq_ignore_ascii_case(normalize_host(actual))
}

fn normalize_host(host: &str) -> &str {
    let normalized = host
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');
    normalized
        .split_once(':')
        .map(|(host, _port)| host)
        .unwrap_or(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_remote_url(remote: &str, hosts: &[&str], expected: Option<&str>) {
        let hosts = hosts
            .iter()
            .map(|host| (*host).to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            github_web_url_from_remote(remote, &hosts).as_deref(),
            expected
        );
    }

    #[test]
    fn test_github_remote_url_parsing() {
        for remote in [
            "git@github.com:owner/repo.git",
            "https://github.com/owner/repo.git",
            "ssh://git@github.com/owner/repo.git",
            "git+ssh://git@github.com/owner/repo.git",
            "ssh://deploy@github.com:22/owner/repo.git",
            "github.com:owner/repo.git",
        ] {
            assert_remote_url(
                remote,
                &["github.com"],
                Some("https://github.com/owner/repo"),
            );
        }
        assert_remote_url("git@example.com:owner/repo.git", &["github.com"], None);
        // Shell metacharacters in owner/repo must not reach the OS URL opener
        assert_remote_url("https://github.com/owner/re&po.git", &["github.com"], None);
        assert_remote_url("git@github.com:own er/repo.git", &["github.com"], None);
    }

    #[test]
    fn test_github_enterprise_remote_url_parsing() {
        assert_remote_url(
            "git@git.example.com:team/repo.git",
            &["git.example.com"],
            Some("https://git.example.com/team/repo"),
        );
        assert_remote_url(
            "https://github.com/team/repo.git",
            &["git.example.com"],
            None,
        );
    }

    #[test]
    fn test_github_repo_name_from_web_url() {
        assert_eq!(
            github_repo_name_from_web_url("https://github.com/owner/repo"),
            Some("repo")
        );
        assert_eq!(
            github_repo_name_from_web_url("https://github.com/owner/repo/"),
            Some("repo")
        );
        assert_eq!(github_repo_name_from_web_url(""), None);
    }

    #[test]
    fn test_github_remote_url_ignores_fetch_only_remotes() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();
        repo.remote("genesisone", "https://github.com/owner/GenesisOne.git")
            .unwrap();
        repo.remote_set_pushurl("genesisone", Some("DISABLED"))
            .unwrap();

        assert_eq!(github_remote_url(&repo, &["github.com".into()]), None);

        repo.remote("origin", "https://github.com/owner/BiteMoment.git")
            .unwrap();
        assert_eq!(
            github_remote_url(&repo, &["github.com".into()]).as_deref(),
            Some("https://github.com/owner/BiteMoment")
        );
    }
}
