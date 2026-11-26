// SPDX-FileCopyrightText: 2026 Oleg Smirnov
// SPDX-License-Identifier: MIT

//! The command run the way a user runs it.

use std::process::Command;

fn cmslens() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cmslens"))
}

/// The help text printed the ScraperAPI key from the environment as the flag's
/// default value: a terminal, a log or a screenshot of `cmslens -h` gave the
/// key away.
#[test]
fn help_does_not_show_the_key() {
    let key = "test-key-not-for-real-use";
    let out = cmslens()
        .arg("-h")
        .env("SCRAPERAPI_KEY", key)
        .output()
        .unwrap();
    assert!(out.status.success(), "cmslens -h: {out:?}");
    let text = String::from_utf8(out.stderr).unwrap();
    assert!(!text.contains(key), "the help text shows the key:\n{text}");
    assert!(
        text.contains("-scraperapi-key"),
        "the help text lost the flag:\n{text}"
    );
}

/// A saved page is read without the network, and the JSON line has the shape
/// the Go build printed: HTML characters escaped, an empty list rather than
/// null.
#[test]
fn file_is_detected_offline() {
    let dir = std::env::temp_dir().join(format!("cmslens-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let page = dir.join("page.html");
    std::fs::write(
        &page,
        r#"<html><head><meta name="generator" content="WordPress 6.6"></head></html>"#,
    )
    .unwrap();
    let blank = dir.join("blank&.html");
    std::fs::write(&blank, "<html></html>").unwrap();

    let out = cmslens().arg("-file").arg(&page).output().unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        format!("{}\tWordPress\n", page.display())
    );

    let out = cmslens()
        .args(["-json", "-file"])
        .arg(&blank)
        .output()
        .unwrap();
    let want = format!(
        "{{\"input\":\"{}\",\"result\":{{\"status\":\"ok\",\"detected\":[],\"wafBehind\":false,\"details\":{{\"server\":\"\",\"statusCode\":0}}}}}}\n",
        blank.display().to_string().replace('&', "\\u0026")
    );
    assert_eq!(String::from_utf8(out.stdout).unwrap(), want);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// An input that is not a domain fails before any request, with exit code 1
/// and the error in the line.
#[test]
fn invalid_input_fails_without_network() {
    let out = cmslens().args(["-json", "not a host"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "{\"input\":\"not a host\",\"result\":{\"status\":\"\",\"detected\":[],\"wafBehind\":false,\"details\":{\"server\":\"\",\"statusCode\":0}},\"error\":\"not a domain name: \\\"not a host\\\"\"}\n"
    );
    let out = cmslens().output().unwrap();
    assert_eq!(out.status.code(), Some(2), "no domains is a usage error");
}

/// The version line has the form of the release archive and of the Go build,
/// `cmslens 0.2.0`, with no leading `v`, so a bug report quotes the same line
/// whichever way the command was installed.
#[test]
fn version_has_one_form() {
    let out = cmslens().arg("-version").output().unwrap();
    assert!(out.status.success(), "cmslens -version: {out:?}");
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        format!("cmslens {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(
        env!("CARGO_PKG_VERSION").starts_with(|c: char| c.is_ascii_digit()),
        "the version starts with a digit"
    );
}
