use geohash::{encode, decode, neighbor, Direction, Coord};
//use std::borrow::Borrow;
use std::collections::BTreeMap;
use std::vec::Vec;
use digest::Digest;
use sha2::Sha256;
use std::cell::RefCell;

const EARTH_RADIUS: f64 = 6_371_000.0;

type GeoIndex = BTreeMap<[u8; 32],Vec<String>>; //Vec<[u8; 32]>
type GeoHashLookup = BTreeMap<String,String>;

thread_local! {
    static GEO_INDEX: RefCell<GeoIndex> = RefCell::default();
    static GEO_HASH_LOOKUP: RefCell<GeoHashLookup> = RefCell::default();
}

fn get_id(s_id: &String) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(s_id.as_bytes());
    let result = hasher.finalize();
    let mut hash = [0; 32];
    hash.copy_from_slice(&result);
    hash
}

fn encode_coords(c: Coord, size: usize) -> String {
    match encode(c, size){
        Err(_) => String::new(),
        Ok(c) => c
    }
}

fn _index_lookup(geohash: &String, id:&String){
    GEO_HASH_LOOKUP.with(|geo_hash_lookup|{
        geo_hash_lookup.borrow_mut().insert(id.to_string(),geohash.to_string());
    })
}


pub fn lookup(id: &String) -> String{
    let _id = get_id(id);
    GEO_HASH_LOOKUP.with(|geo_hash_lookup|{
        let _geo_hash_lookup = geo_hash_lookup.borrow();
        let result = _geo_hash_lookup.get(id);
        match result{
            Some(geohash)=>{
                geohash.to_string()
            },
            None => {
                panic!("element does not exist");
            }
        }
    })
}


fn _index(geohash_ar: Vec<String>, id:&String ) { //
    GEO_INDEX.with(|geo_index|{
        let mut index_mut = geo_index.borrow_mut();
        for geohash in geohash_ar{
            let key = get_id(&geohash);
            if index_mut.contains_key(&key){        
                let v = index_mut.get_mut(&key).unwrap();
                let find = v.iter().find(|&s| s == id);
                match find{
                    Some(_)=>{},
                    None => {
                        v.push(id.to_string());
                    }
                }
                
            } else {
                let mut v: Vec<String> = Vec::new();
                v.push(id.to_string());
                index_mut.insert(key, v);
            }
        }
        
    })
}

fn get(geohash: String) -> Vec<String>{
    let empty_vec: &Vec<String> = &Vec::new();
    GEO_INDEX.with(|geo_index|{
        let key = get_id(&geohash);
        let _index = geo_index.borrow();
        let val: &Vec<String> = _index.get(&key).unwrap_or_else(||{empty_vec});
        let mut ret: Vec<String> = Vec::new();
        for v in val{
            ret.push(v.to_string());
        }
        ret
        
    })
}


fn get_precision(distance: &f64) -> usize{
     /*
        1: ± 5,009 km x 4,992 km
        2: ± 1,252 km x 624 km
        3: ± 156 km x 156 km
        4: ± 39.1 km x 19.5 km
        5: ± 4.9 km x 4.9 km
        6: ± 1.2 km x 609 m
        7: ± 152 m x 152 m
        8: ± 38 m x 19 m
        9: ± 4.8 m x 4.8 m
        10: ± 1.2 m x 59.5 cm
    */
    let distance = *distance; 
    if distance > 156.0 && distance < 1252.0 {
        2
    } else if distance > 39.0 && distance < 156.0 {
        3
    } else if distance > 4.9 && distance < 39.0 {
        4
    } else if distance > 1.2 && distance < 4.9 {
        5
    } else if distance < 1.2 {
        6
    } else {
        1
    }
}


fn haversine(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();

    let a = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());

    EARTH_RADIUS * c
}

fn get_distance(coord1: &Coord, geohash2: &String) -> f64{
    let (coord2, _, _) = decode(geohash2).unwrap();
    haversine(coord1.x, coord1.y, coord2.x, coord2.y)/1000.0 //returns distance in kilometers
}

pub fn get_distance_from_geohash(geohash1: String, geohash2: String) -> f64{
    let (c,_,_) = decode(&geohash1).unwrap();
    get_distance(&c,&geohash2)
}

fn in_radius(c: &Coord, radius: &f64, id: &String) -> bool{
    let geohash2 = lookup(id);
    let dist = get_distance(c,&geohash2);
    dist<=radius.clone()
}

pub fn index(geohash: String, id: String) {
    //let id = get_id(&id);
    let (c,_,_) = decode(&geohash).unwrap();
    let to_index: Vec<String> = vec![
        encode_coords(c.clone(),1),
        encode_coords(c.clone(),2),
        encode_coords(c.clone(),3),
        encode_coords(c.clone(),4),
        encode_coords(c.clone(),5),
        encode_coords(c.clone(),6)
    ];
    
    _index(to_index,&id);
    _index_lookup(&geohash,&id);

}

pub fn view_index() -> Vec<String>{
    let mut empty_vec: Vec<String> = Vec::new();
    GEO_INDEX.with(|geo_index|{
        let data_map = geo_index.borrow();
        //let data_map_ref = data_map.borrow();
        for (_, value) in data_map.iter(){
            for item in value.iter(){
                empty_vec.push(item.clone());
            }
        }
       empty_vec
    })
}

pub fn find(geohash: String, distance: f64) -> Vec<String>{ //distance is in kilometers
    let (c,_,_) = decode(&geohash).unwrap();
    let prec = get_precision(&distance);
    let _geohash = encode_coords(c.clone(),prec);
    let mut ret: Vec<String> = Vec::new();
    let directions: Vec<Direction> = vec![
        Direction::N,
        Direction::NE,
        Direction::E,
        Direction::SE,
        Direction::S,
        Direction::SW,
        Direction::W,
        Direction::NW
    ];
    let _ids = get(_geohash.clone());
    for id in _ids{
        if in_radius(&c,&distance,&id) {
            ret.push(id);
        }
    }
    for direction in &directions {
        let _neighbor = neighbor(&_geohash, *direction);
        match _neighbor{
            Ok(n)=>{
                let _ids = get(n);
                for id in _ids{
                    if in_radius(&c,&distance,&id){
                        ret.push(id);
                    }
                }
            },
            Err(_)=>{}
        }
    }
    ret

}
