use std::collections::{HashSet, BTreeMap};
use std::cmp::Reverse;
use std::io;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use dir_signature::v1::merge::MergedSignatures;
use dir_signature::v1::{Entry, EntryKind, Hashes};

use ciruela::VPath;
use metadata::{Meta, Error};
use metadata::{read_index, scan};
use tracking::Index;


#[derive(Debug)]
pub struct Hardlink {
    pub source: VPath,
    pub path: PathBuf,
    pub exe: bool,
    pub size: u64,
    pub hashes: Hashes,
}


pub fn files_to_link(index: Index, dir: VPath, meta: Meta)
    -> Result<Vec<Hardlink>, Error>
{
    let all_states = match meta.signatures()?.open_vpath(&dir) {
        Ok(dir) => scan::all_states(&dir)?,

        Err(Error::Open(_, ref e))
        if e.kind() == io::ErrorKind::NotFound
        => BTreeMap::new(),

        Err(e) => return Err(e.into()),
    };
    // deduplicate, assuming all images really exist
    let mut vec = all_states.into_iter().collect::<Vec<_>>();
    vec.sort_unstable_by_key(|&(_, ref s)| {
        Reverse(
            s.signatures.iter()
            .map(|s| s.timestamp).max()
            .unwrap_or(UNIX_EPOCH))
    });
    let mut visited = HashSet::new();
    let mut selected = Vec::new();
    for (dir, s) in vec {
        if visited.contains(&s.image) {
            continue;
        }
        visited.insert(s.image.clone());
        selected.push((dir, s));
        if selected.len() > 36 {  // TODO(tailhook) make tweakable
            break;
        }
    }
    let mut files = Vec::new();
    for (dir_name, s) in selected {
        // TODO(tailhook) look in cache
        match read_index::open(&s.image, &meta) {
            Ok(index) => {
                files.push((dir.join(dir_name), index));
            }
            Err(ref e) => {
                warn!("Error reading index {:?} from file: {}", s.image, e);
            }
        }
    }
    let mut result = Vec::new();
    let mut msig = MergedSignatures::new(files)?;
    let mut msig_iter = msig.iter();
    for entry in index.entries.iter() {
        match entry {
            &Entry::File {
                path: ref lnk_path,
                exe: lnk_exe,
                size: lnk_size,
                hashes: ref lnk_hashes,
            } => {
                for tgt_entry in msig_iter.advance(&EntryKind::File(lnk_path))
                {
                    match tgt_entry {
                        (vpath,
                         Ok(Entry::File {
                             path: ref tgt_path,
                             exe: tgt_exe,
                             size: tgt_size,
                             hashes: ref tgt_hashes
                        }))
                        if lnk_exe == tgt_exe &&
                            lnk_size == tgt_size &&
                            lnk_hashes == tgt_hashes
                        => {
                            debug_assert_eq!(tgt_path, lnk_path);
                            result.push(Hardlink {
                                source: vpath.clone(),
                                path: lnk_path.clone(),
                                exe: lnk_exe,
                                size: lnk_size,
                                hashes: lnk_hashes.clone(),
                            });
                            break;
                        },
                        _ => continue,
                    }
                }
            },
            _ => {},
        }
    }
    Ok(result)
}
