//! Skill management commands.
//!
//! Lists and reloads skills via the agent gRPC service over Unix domain socket.

use anyhow::{Context, Result};

use super::common::{connect_agent, proto};
use proto::{ListSkillsRequest, ReloadSkillsRequest};

/// List available skills by querying the agent.
pub async fn list(socket: &str) -> Result<()> {
    let mut client = connect_agent(socket).await?;

    let resp = client
        .list_skills(ListSkillsRequest {
            user_id: String::new(),
            conversation_id: None,
        })
        .await
        .context("failed to list skills")?;

    let skills = resp.into_inner().skills;

    if skills.is_empty() {
        println!("No skills found.");
        return Ok(());
    }

    println!("{:<30} DESCRIPTION", "NAME");
    println!("{}", "-".repeat(80));

    for skill in &skills {
        println!("{:<30} {}", skill.name, truncate(&skill.description, 48),);
    }

    println!("\n{} skill(s) total.", skills.len());
    Ok(())
}

/// Reload the skill catalog via the agent.
pub async fn reload(socket: &str) -> Result<()> {
    let mut client = connect_agent(socket).await?;

    let resp = client
        .reload_skills(ReloadSkillsRequest {
            conversation_id: None,
            user_id: None,
        })
        .await
        .context("failed to reload skills")?;

    let skills = resp.into_inner().skills;
    println!(
        "Skill catalog reloaded. {} skill(s) available.",
        skills.len()
    );
    Ok(())
}

/// Truncate a string to `max` chars, appending "..." if truncated.
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_owned()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}
