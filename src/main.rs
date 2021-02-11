use std::process::Command;
use std::io;
use reqwest::blocking::Client;
use reqwest::header::HeaderMap;
use std::collections::HashMap;
use std::error::Error;
use reqwest::header::AUTHORIZATION;
use reqwest::header::CONTENT_TYPE;
use reqwest::header::HeaderValue;
use base64;
use regex::Regex;
use serde::Deserialize;
use std::fs::File;
use std::io::Read;
use std::io::Write;
use std::env::args_os;
use std::env::args;
use std::env::current_dir;
use std::ffi::OsString;

fn get_commit_message(commit: &str) -> io::Result<String> {
    let output = Command::new("git").arg("show").arg("--format=\"%B\"").arg("-s").arg(commit).output()?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string().trim().trim_matches('\"').trim().to_string();
    Ok(stdout)
}

fn get_commit_url_root() -> io::Result<String> {
    let output = Command::new("git").arg("remote").arg("get-url").arg("origin").output()?;
    let mut url = String::from_utf8_lossy(&output.stdout).to_string();
    url = url.replace(":", "/").replace("git@", "https://").replace(".git", "").trim().to_string() + "/commit/";
    Ok( url )
}

fn push(args: Vec<OsString>) -> io::Result<Vec<(String, String)>> {
    let output = Command::new("git").arg("push").arg("--porcelain").args(args).output()?;
    io::stdout().write_all(&output.stdout)?;
    io::stderr().write_all(&output.stderr)?;
    let stdout_string = String::from_utf8_lossy(&output.stdout).to_string();
    
    let re = Regex::new(r"\t[^\t]+\t([0-9a-f]{6,20})\.\.([0-9a-f]{6,20})").unwrap(); //.[0-9a-f]{7}$
    
    Ok(re.captures_iter(&stdout_string).map(|pushed_hash_match| {
        let hash_from = pushed_hash_match.get(1).unwrap().as_str();
        let hash_to = pushed_hash_match.get(2).unwrap().as_str();
        (hash_from.to_string(), hash_to.to_string())
    }).collect())
}


// https://id.atlassian.com/manage/api-tokens
fn post_comment(config: &Config, issue: &str, message: &str) -> Result<(), Box<dyn Error>>{
    let client = Client::new();
    
    let mut map = HashMap::new();
    map.insert("body", message);

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(AUTHORIZATION, HeaderValue::from_str(&format!("Basic {}", base64::encode(&format!("{}:{}", config.username, config.token)))).unwrap());
    let req = client.post(&format!("https://{}/rest/api/2/issue/{}/comment", config.host, issue))
        .headers(headers)
        .json(&map);
    let res = req.send()?;
    Ok( () )
}

#[derive(Deserialize)]
struct Config {
    host: String,
    username: String,
    token: String,
}

fn hashes_in_range(from: &str, to: &str) -> io::Result<Vec<String>> {
    let output = Command::new("git").arg("log").arg("--format=\"%H\"").arg(format!("{}..{}", from, to)).output()?;
    let stdout_string = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(stdout_string.lines().map(|line| line.trim_matches('\"').to_string()).collect())
}

fn comment_for_hash(config: &Config, hash: &str) {
    let message = get_commit_message(&hash).unwrap();
    
    let github_url = format!("{}{}", get_commit_url_root().unwrap(), hash);
    let re = Regex::new(r"[A-Z]{2,6}-\d+").unwrap();
    let comment_body = format!("[Commit {shorthash}|{github_url}]: {message}", shorthash = &hash[..7], github_url = github_url, message = message);
    
    for task in re.captures_iter(&message) {
        let task_name: &str = task.get(0).unwrap().as_str();
        println!("Adding commit comment for {} to {}", hash, task_name);
        let comment_body_without_redundant_task = comment_body.replace(task_name, "");
        post_comment(&config, &task_name, &comment_body_without_redundant_task).unwrap();
    }
}

fn open_config_file() -> io::Result<File> {
    let mut dir = current_dir()?;
    dir.push(".jira-push");
    
    loop {
        match File::open(&dir) {
            Ok(f) => { return Ok(f); }
            Err(e) => {
                if !dir.pop() {
                    return Err(e)
                }
            }
        }
    }
}

fn main() {
    if args().skip(1).next().map(|x| x=="--help").unwrap_or(false) {
        println!("Usage: create a file name .jira-push in the repository root. It contents should look like:");
        println!("");
        println!("host = \"yourdomain.atlassian.net\"");
        println!("username = \"yourusername@your-organization.com\"");
        println!("token = \"a_jira_api_token_for_your_account\"");
        println!("");
        println!("The token can be generated at https://id.atlassian.com/manage/api-tokens");
        return;
    }

    let config: Config = {
        let mut config_file = open_config_file().expect("Could not open config file (.jira-push)");
        let mut config_str = String::new();
        config_file.read_to_string(&mut config_str).expect("Reading config file (.jira-push) failed");
        toml::from_str(&config_str).expect("Invalid config file (.jira-push) contents")
    };
    
    if args().skip(1).next().map(|x| x=="--hash").unwrap_or(false) && args().len()==3 {
        let hash = args().skip(2).next().expect("Missing hash");
        comment_for_hash(&config, &hash);
        return;
    }
    
    let pushed_ranges = push(args_os().skip(1).collect()).expect("push failed");
    
    for (from_hash, to_hash) in pushed_ranges {
        for hash in hashes_in_range(&from_hash, &to_hash).expect("Listing hashes in range failed") {
            comment_for_hash(&config, &hash);
        }
    }
}
