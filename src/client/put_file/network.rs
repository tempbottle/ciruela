use std::sync::Arc;
use std::time::{SystemTime, Duration};
use std::fs::read;

use abstract_ns::Name;
use failure::{Error, ResultExt};
use futures::Future;
use futures::future::{err, join_all, Either};
use tk_easyloop::{self, handle};
use ns_env_config;
use ssh_keys::PrivateKey;

use {VPath};
use ciruela::blocks::ThreadedBlockReader;
use ciruela::index::InMemoryIndexes;
use ciruela::cluster::{Config, Connection};
use ciruela::signature::sign_upload;

use sync::network::upload_with_progress;
use put_file::PutFileOptions;


pub fn put(config: Arc<Config>, clusters: Vec<Vec<Name>>,
    keys: Vec<PrivateKey>,
    indexes: &InMemoryIndexes, blocks: &ThreadedBlockReader,
    opts: PutFileOptions)
    -> Result<(), Error>
{
    if clusters.len() == 0 {
        bail!("at least one destination host name is expected");
    }
    let data = read(&opts.source_file)?;
    let res = tk_easyloop::run(|| {
        let ns = ns_env_config::init(&handle()).expect("init dns");
        let conns = clusters.iter().map(|addr| {
            Connection::new(addr.clone(),
                ns.clone(), indexes.clone(), blocks.clone(), &config)
        }).collect::<Vec<_>>();
        let vpath = VPath::from(&opts.dir);
        conns[0].fetch_index(&vpath)
        .then(|res| res.context("can't fetch index").map_err(Error::from))
        .and_then(|idx| {
            let hash = idx.get_hash()
                .context("can't parse index").map_err(Error::from)?;
            let idx = idx.into_mut()
                .context("can't parse index").map_err(Error::from)?;
            return Ok((hash, idx));
        })
        .and_then(move |(old_image, idx)| {
            let mut idx = idx;
            match idx.insert_file(&opts.file, &data[..], false) {
                Ok(()) => {}
                Err(e) => return Either::B(err(e.into())),
            }
            blocks.register_memory_blocks(
                idx.hash_type(), idx.block_size(), data);
            let new_index = idx.to_raw_data();
            let image_id = indexes.register_index(&new_index)
                .expect("index is valid");
            let upload = sign_upload(&vpath,
                &image_id, SystemTime::now(), &keys);
            Either::A(join_all(conns.into_iter().map(move |conn| {
                let up = conn.replace_if_matches(
                    upload.clone(), old_image.clone());
                upload_with_progress(up, Duration::new(30,0))
                    .map_err(Into::into)
            })).map(Either::A))
        })
    })?;
    match res {
        Either::A(results) => {
            for res in results {
                println!("{}", res);
            }
        }
        Either::B(()) => warn!("file is unchanged."),
    }

    Ok(())
}
