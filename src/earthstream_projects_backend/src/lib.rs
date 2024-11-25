use candid::{CandidType, Principal};
use ic_cdk::caller;
use ic_cdk_macros::*;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::{HashMap, BTreeMap};
use std::cmp::min;

mod geo_index;

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ProjectStatus {
    PendingReview,
    Approved,
    Rejected,
    Suspended
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum GatewayType {
    Wifi,
    GSM
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug)]
pub struct ProjectImages {
    background: String,
    gallery: Vec<String>
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug)]
pub struct Location {
    lat: f64,
    lng: f64,
    address: String,
    geohash: String,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug)]
pub struct Project {
    id: String,
    name: String,
    description: String,
    gateway_type: GatewayType,
    images: ProjectImages,
    location: Location,
    project_discord: Option<String>,
    private_discord: String,
    sensors_required: u32,
    video: Option<String>,
    status: ProjectStatus,
    owner: Principal,
    created_at: u64,
    vote_count: u64,  // Cache for quick access to vote count
    featured: bool,
    featured_at: Option<u64>,
    tags: Vec<String>,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug)]
pub struct Vote {
    voter: Principal,
    timestamp: u64,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug)]
pub struct ProjectsResponse {
    projects: Vec<Project>,
    total: u64,
    page: u32,
    pages: u32,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug)]
pub struct ProjectData {
    name: String,
    description: String,
    gateway_type: GatewayType,
    images: ProjectImages,
    location: Location,
    project_discord: Option<String>,
    private_discord: String,
    sensors_required: u32,
    video: Option<String>,
    tags: Vec<String>,
}

struct State {
    projects: HashMap<String, Project>,
    admins: HashMap<Principal, bool>,  // bool for is_super_admin
    owner_projects: HashMap<Principal, Vec<String>>,
    date_index: BTreeMap<u64, String>,
    project_votes: HashMap<String, HashMap<Principal, Vote>>,
    vote_index: HashMap<Principal, Vec<String>>,  // User's voted projects
    featured_projects: BTreeMap<u64, String>,  // timestamp -> project_id
    tag_index: HashMap<String, Vec<String>>,   // tag -> project_ids
}

impl Default for State {
    fn default() -> Self {
        Self {
            projects: HashMap::new(),
            admins: HashMap::new(),
            owner_projects: HashMap::new(),
            date_index: BTreeMap::new(),
            project_votes: HashMap::new(),
            vote_index: HashMap::new(),
            featured_projects: BTreeMap::new(),
            tag_index: HashMap::new(),
        }
    }
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

// Helper functions
fn index_text(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split_whitespace()
        .map(|s| s.to_string())
        .collect()
}

fn caller_is_super_admin() -> bool {
    let caller = caller();
    STATE.with(|state| {
        state.borrow()
            .admins
            .get(&caller)
            .copied()
            .unwrap_or(false)
    })
}

fn caller_is_admin() -> bool {
    let caller = caller();
    STATE.with(|state| state.borrow().admins.contains_key(&caller))
}

fn generate_project_id(name: &str, owner: &Principal, timestamp: u64) -> String {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(name.as_bytes());
    hasher.update(owner.to_string().as_bytes());
    hasher.update(timestamp.to_string().as_bytes());
    format!("{:x}", hasher.finalize())
}

fn paginate<T: Clone>(items: Vec<T>, page: Option<u32>, limit: Option<u32>) -> (Vec<T>, u64, u32) {
    let limit = limit.unwrap_or(20) as usize;
    let page = page.unwrap_or(1) as usize;
    let total_items = items.len();
    let total_pages = (total_items + limit - 1) / limit;
    let start = (page - 1) * limit;
    let end = min(start + limit, total_items);
    
    (
        items[start..end].to_vec(),
        total_items as u64,  // Convert to u64 here
        total_pages as u32
    )
}

// Admin Management
#[update]
fn create_super_admin() -> Result<(), String> {
    let caller = caller();
    if caller == Principal::anonymous() {
        return Err("Anonymous principals cannot be admins".to_string());
    }

    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.admins.is_empty() {
            state.admins.insert(caller, true);
            Ok(())
        } else {
            Err("Super admin already exists".to_string())
        }
    })
}

#[update]
fn add_admin(principal: Principal) -> Result<(), String> {
    if !caller_is_super_admin() {
        return Err("Only super admin can add admins".to_string());
    }
    
    if principal == Principal::anonymous() {
        return Err("Cannot add anonymous principal as admin".to_string());
    }

    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.admins.insert(principal, false);
        Ok(())
    })
}

#[update]
fn remove_admin(principal: Principal) -> Result<(), String> {
    if !caller_is_super_admin() {
        return Err("Only super admin can remove admins".to_string());
    }

    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.admins.get(&principal) == Some(&true) {
            return Err("Cannot remove super admin".to_string());
        }
        state.admins.remove(&principal);
        Ok(())
    })
}

// Project Management
#[update]
fn create_project(project_data: ProjectData) -> Result<String, String> {
    let caller = caller();
    if caller == Principal::anonymous() {
        return Err("Anonymous principals cannot create projects".to_string());
    }

    let timestamp = ic_cdk::api::time();
    let project_id = generate_project_id(&project_data.name, &caller, timestamp);

    let project = Project {
        id: project_id.clone(),
        name: project_data.name,
        description: project_data.description,
        gateway_type: project_data.gateway_type,
        images: project_data.images,
        location: project_data.location.clone(),
        project_discord: project_data.project_discord,
        private_discord: project_data.private_discord,
        sensors_required: project_data.sensors_required,
        video: project_data.video,
        status: ProjectStatus::PendingReview,
        owner: caller,
        created_at: timestamp,
        vote_count: 0,
        featured: false,
        featured_at: None,
        tags: project_data.tags.clone(),
    };

    STATE.with(|state| {
        let mut state = state.borrow_mut();
        
        // Store project
        state.projects.insert(project_id.clone(), project);
        
        // Update owner index
        state.owner_projects
            .entry(caller)
            .or_insert_with(Vec::new)
            .push(project_id.clone());
        
        // Update date index
        state.date_index.insert(timestamp, project_id.clone());
        
        // Index location
        geo_index::index(project_data.location.geohash, project_id.clone());
        for tag in &project_data.tags {
            state.tag_index
                .entry(tag.to_lowercase())
                .or_insert_with(Vec::new)
                .push(project_id.clone());
        }

    });

    Ok(project_id)
}

#[update]
fn update_project(id: String, project_data: ProjectData) -> Result<(), String> {
    let caller = caller();
    
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        
        let project = state.projects.get_mut(&id)
            .ok_or("Project not found")?;
        
        if project.owner != caller {
            return Err("Only project owner can update".to_string());
        }

        // Update fields
        project.name = project_data.name;
        project.description = project_data.description;
        project.gateway_type = project_data.gateway_type;
        project.images = project_data.images;
        project.location = project_data.location.clone();
        project.project_discord = project_data.project_discord;
        project.private_discord = project_data.private_discord;
        project.sensors_required = project_data.sensors_required;
        project.video = project_data.video;

        // Update geohash index
        geo_index::index(project_data.location.geohash, id);
        
        Ok(())
    })
}

#[update]
fn update_project_status(id: String, status: ProjectStatus) -> Result<(), String> {
    if !caller_is_admin() {
        return Err("Only admins can update project status".to_string());
    }

    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let project = state.projects.get_mut(&id)
            .ok_or("Project not found")?;
        project.status = status;
        Ok(())
    })
}

#[update]
fn feature_project(project_id: String) -> Result<(), String> {
    if !caller_is_admin() {
        return Err("Only admins can feature projects".to_string());
    }

    let timestamp = ic_cdk::api::time();

    STATE.with(|state| {
        let mut state = state.borrow_mut();
        
        // First check if project exists and is not already featured
        if let Some(project) = state.projects.get(&project_id) {
            if project.featured {
                return Err("Project is already featured".to_string());
            }
        } else {
            return Err("Project not found".to_string());
        }
        
        // Then update the project
        if let Some(project) = state.projects.get_mut(&project_id) {
            project.featured = true;
            project.featured_at = Some(timestamp);
        }
        
        // Finally update the featured projects index
        state.featured_projects.insert(timestamp, project_id);
        
        Ok(())
    })
}

#[update]
fn unfeature_project(project_id: String) -> Result<(), String> {
    if !caller_is_admin() {
        return Err("Only admins can unfeature projects".to_string());
    }

    STATE.with(|state| {
        let mut state = state.borrow_mut();
        
        // First get the featured_at timestamp and check if project is featured
        let featured_at = if let Some(project) = state.projects.get(&project_id) {
            if !project.featured {
                return Err("Project is not featured".to_string());
            }
            project.featured_at
        } else {
            return Err("Project not found".to_string());
        };
        
        // Remove from featured_projects if we have a timestamp
        if let Some(timestamp) = featured_at {
            state.featured_projects.remove(&timestamp);
        }
        
        // Update the project
        if let Some(project) = state.projects.get_mut(&project_id) {
            project.featured = false;
            project.featured_at = None;
        }
        
        Ok(())
    })
}

// Voting System
#[update]
fn vote_for_project(project_id: String) -> Result<(), String> {
    let caller = caller();
    if caller == Principal::anonymous() {
        return Err("Anonymous principals cannot vote".to_string());
    }

    STATE.with(|state| {
        let mut state = state.borrow_mut();
        
        // Verify project exists
        if !state.projects.contains_key(&project_id) {
            return Err("Project not found".to_string());
        }

        let vote = Vote {
            voter: caller,
            timestamp: ic_cdk::api::time(),
        };

        // Add vote
        state.project_votes
            .entry(project_id.clone())
            .or_insert_with(HashMap::new)
            .insert(caller, vote);

        // Update vote index
        state.vote_index
            .entry(caller)
            .or_insert_with(Vec::new)
            .push(project_id.clone());

        // Update vote count
        if let Some(project) = state.projects.get_mut(&project_id) {
            project.vote_count += 1;
        }

        Ok(())
    })
}

#[update]
fn remove_vote(project_id: String) -> Result<(), String> {
    let caller = caller();

    STATE.with(|state| {
        let mut state = state.borrow_mut();
        
        // Remove vote from project_votes
        if let Some(votes) = state.project_votes.get_mut(&project_id) {
            if votes.remove(&caller).is_none() {
                return Err("No vote found".to_string());
            }
        } else {
            return Err("Project not found".to_string());
        }

        // Remove from vote index
        if let Some(voted_projects) = state.vote_index.get_mut(&caller) {
            voted_projects.retain(|id| id != &project_id);
        }

        // Update vote count
        if let Some(project) = state.projects.get_mut(&project_id) {
            project.vote_count = project.vote_count.saturating_sub(1);
        }

        Ok(())
    })
}

// Query functions
#[query]
fn get_project(id: String) -> Option<Project> {
    STATE.with(|state| {
        state.borrow().projects.get(&id).cloned()
    })
}

#[query]
fn get_projects_by_ids(ids: Vec<String>, page: Option<u32>, limit: Option<u32>) -> ProjectsResponse {
    STATE.with(|state| {
        let state = state.borrow();
        let projects: Vec<Project> = ids.iter()
            .filter_map(|id| state.projects.get(id))
            .cloned()
            .collect();
        
        let (paginated_projects, total, pages) = paginate(projects, page, limit);
        
        ProjectsResponse {
            projects: paginated_projects,
            total,  // Now this is u64
            page: page.unwrap_or(1),
            pages,
        }
    })
}

#[query]
fn get_projects_by_owner(owner: Principal, page: Option<u32>, limit: Option<u32>) -> ProjectsResponse {
    STATE.with(|state| {
        let state = state.borrow();
        let projects: Vec<Project> = state.owner_projects
            .get(&owner)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| state.projects.get(id))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        
        let (paginated_projects, total, pages) = paginate(projects, page, limit);
        
        ProjectsResponse {
            projects: paginated_projects,
            total,
            page: page.unwrap_or(1),
            pages,
        }
    })
}

#[query]
fn get_projects_by_date_range(start: u64, end: u64, page: Option<u32>, limit: Option<u32>) -> ProjectsResponse {
    STATE.with(|state| {
        let state = state.borrow();
        let projects: Vec<Project> = state.date_index
            .range(start..=end)
            .filter_map(|(_, id)| state.projects.get(id))
            .cloned()
            .collect();
        
        let (paginated_projects, total, pages) = paginate(projects, page, limit);
        
        ProjectsResponse {
            projects: paginated_projects,
            total,
            page: page.unwrap_or(1),
            pages,
        }
    })
}

#[query]
fn get_projects_by_location(lat: f64, lng: f64, radius: f64) -> Vec<Project> {
    STATE.with(|state| {
        let state = state.borrow();
        let project_ids = geo_index::find(format!("{},{}", lat, lng), radius);
        project_ids.iter()
            .filter_map(|id| state.projects.get(id))
            .cloned()
            .collect()
    })
}

#[query]
fn get_project_votes(project_id: String) -> u64 {
    STATE.with(|state| {
        state.borrow()
            .projects
            .get(&project_id)
            .map(|p| p.vote_count)
            .unwrap_or(0)
    })
}

#[query]
fn get_user_vote_for_project(project_id: String, user: Principal) -> bool {
    STATE.with(|state| {
        state.borrow()
            .project_votes
            .get(&project_id)
            .map(|votes| votes.contains_key(&user))
            .unwrap_or(false)
    })
}

#[query]
fn get_user_voted_projects(user: Principal, page: Option<u32>, limit: Option<u32>) -> ProjectsResponse {
    STATE.with(|state| {
        let state = state.borrow();
        let projects: Vec<Project> = state.vote_index
            .get(&user)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| state.projects.get(id))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        
        let (paginated_projects, total, pages) = paginate(projects, page, limit);
        
        ProjectsResponse {
            projects: paginated_projects,
            total,
            page: page.unwrap_or(1),
            pages,
        }
    })
}

#[query]
fn get_projects_by_gateway_type(gateway_type: GatewayType, page: Option<u32>, limit: Option<u32>) -> ProjectsResponse {
    STATE.with(|state| {
        let state = state.borrow();
        let projects: Vec<Project> = state.projects
            .values()
            .filter(|p| p.gateway_type == gateway_type)
            .cloned()
            .collect();
        
        let (paginated_projects, total, pages) = paginate(projects, page, limit);
        
        ProjectsResponse {
            projects: paginated_projects,
            total,
            page: page.unwrap_or(1),
            pages,
        }
    })
}

#[query]
fn get_projects_by_votes(min_votes: Option<u64>, max_votes: Option<u64>, page: Option<u32>, limit: Option<u32>) -> ProjectsResponse {
    STATE.with(|state| {
        let state = state.borrow();
        let mut projects: Vec<Project> = state.projects
            .values()
            .filter(|p| {
                let meets_min = min_votes.map(|min| p.vote_count >= min).unwrap_or(true);
                let meets_max = max_votes.map(|max| p.vote_count <= max).unwrap_or(true);
                meets_min && meets_max
            })
            .cloned()
            .collect();
        
        // Sort by vote count descending
        projects.sort_by(|a, b| b.vote_count.cmp(&a.vote_count));
        
        let (paginated_projects, total, pages) = paginate(projects, page, limit);
        
        ProjectsResponse {
            projects: paginated_projects,
            total,
            page: page.unwrap_or(1),
            pages,
        }
    })
}

#[query]
fn get_featured_projects(page: Option<u32>, limit: Option<u32>) -> ProjectsResponse {
    STATE.with(|state| {
        let state = state.borrow();
        let projects: Vec<Project> = state.featured_projects
            .values()
            .filter_map(|id| state.projects.get(id))
            .cloned()
            .collect();
        
        let (paginated_projects, total, pages) = paginate(projects, page, limit);
        
        ProjectsResponse {
            projects: paginated_projects,
            total,
            page: page.unwrap_or(1),
            pages,
        }
    })
}

// Implement search functionality using index_text:
#[query]
fn search_projects(query: String, page: Option<u32>, limit: Option<u32>) -> ProjectsResponse {
    STATE.with(|state| {
        let state = state.borrow();
        
        // Get search terms
        let search_terms = index_text(&query);
        
        // Search through projects
        let mut projects: Vec<Project> = state.projects
            .values()
            .filter(|project| {
                let project_terms = index_text(&project.name);
                let desc_terms = index_text(&project.description);
                
                // Check if any search term matches project terms
                search_terms.iter().any(|term| 
                    project_terms.contains(term) || desc_terms.contains(term)
                )
            })
            .cloned()
            .collect();
        
        // Sort by relevance (simple implementation - could be improved)
        projects.sort_by(|a, b| {
            let a_name_terms = index_text(&a.name);
            let b_name_terms = index_text(&b.name);
            
            // Count matching terms in name
            let a_matches = search_terms.iter()
                .filter(|term| a_name_terms.contains(term))
                .count();
            let b_matches = search_terms.iter()
                .filter(|term| b_name_terms.contains(term))
                .count();
                
            b_matches.cmp(&a_matches)
        });
        
        let (paginated_projects, total, pages) = paginate(projects, page, limit);
        
        ProjectsResponse {
            projects: paginated_projects,
            total,
            page: page.unwrap_or(1),
            pages,
        }
    })
}

// Add this query function to project.rs

#[query]
fn get_projects_by_status(status: ProjectStatus, page: Option<u32>, limit: Option<u32>) -> ProjectsResponse {
    STATE.with(|state| {
        let state = state.borrow();
        
        // Collect projects with matching status and sort by created_at (newest first)
        let mut projects: Vec<Project> = state.projects
            .values()
            .filter(|p| p.status == status)
            .cloned()
            .collect();
        
        // Sort by created_at timestamp in descending order (newest first)
        projects.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        
        let (paginated_projects, total, pages) = paginate(projects, page, limit);
        
        ProjectsResponse {
            projects: paginated_projects,
            total,
            page: page.unwrap_or(1),
            pages,
        }
    })
}

// Add functionality using get_distance_from_geohash:
#[query]
fn get_nearest_projects(geohash: String, limit: Option<u32>) -> Vec<(Project, f64)> {
    STATE.with(|state| {
        let state = state.borrow();
        let mut projects_with_distance: Vec<(Project, f64)> = state.projects
            .values()
            .map(|project| {
                let distance = geo_index::get_distance_from_geohash(
                    geohash.clone(),
                    project.location.geohash.clone()
                );
                (project.clone(), distance)
            })
            .collect();
        
        // Sort by distance
        projects_with_distance.sort_by(|a, b| 
            a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)
        );
        
        // Take limited number of results
        let limit = limit.unwrap_or(10) as usize;
        projects_with_distance.truncate(limit);
        
        projects_with_distance
    })
}

// Stats and utility queries
#[query]
fn get_total_projects() -> u64 {
    STATE.with(|state| state.borrow().projects.len() as u64)
}

#[query]
fn get_total_votes() -> u64 {
    STATE.with(|state| {
        state.borrow()
            .projects
            .values()
            .map(|p| p.vote_count)
            .sum()
    })
}

#[query]
fn get_index_stats() -> HashMap<String, usize> {
    let mut stats = HashMap::new();
    
    STATE.with(|state| {
        let state = state.borrow();
        let indexed_projects = geo_index::view_index();
        
        stats.insert("total_indexed".to_string(), indexed_projects.len());
        stats.insert("total_projects".to_string(), state.projects.len());
        
        // Count projects by status
        for project in state.projects.values() {
            let status_key = format!("status_{:?}", project.status);
            *stats.entry(status_key).or_insert(0) += 1;
        }
    });
    
    stats
}

#[query]
fn is_admin(principal: Principal) -> bool {
    STATE.with(|state| state.borrow().admins.contains_key(&principal))
}

#[query]
fn is_super_admin(principal: Principal) -> bool {
    STATE.with(|state| {
        state.borrow()
            .admins
            .get(&principal)
            .copied()
            .unwrap_or(false)
    })
}

// Pre-upgrade and post-upgrade hooks for stable storage
#[pre_upgrade]
fn pre_upgrade() {
    // TODO: Implement stable storage
}

#[post_upgrade]
fn post_upgrade() {
    // TODO: Implement stable storage
}