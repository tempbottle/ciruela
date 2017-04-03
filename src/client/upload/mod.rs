use std::collections::HashMap;
use std::io::{self, stdout, stderr};
use std::path::PathBuf;
use std::process::exit;
use std::sync::Arc;
use std::time::{SystemTime, Duration};

use abstract_ns::Resolver;
use futures::future::{Future, Either, join_all, ok};
use futures_cpupool::CpuPool;
use tk_easyloop::{self, timeout};
use argparse::{ArgumentParser};
use dir_signature::{ScannerConfig, HashType, v1, get_hash};

mod options;

use name;
use ciruela::Hash;
use ciruela::time::to_ms;
use ciruela::proto::{SigData, sign};
use global_options::GlobalOptions;
use ciruela::proto::{Client, AppendDir, ImageInfo, BlockPointer};
use ciruela::proto::RequestClient;


fn do_upload(gopt: GlobalOptions, opt: options::UploadOptions)
    -> Result<bool, ()>
{
    let dir = opt.source_directory.clone().unwrap();
    let mut cfg = ScannerConfig::new();
    cfg.hash(HashType::Blake2b_256);
    cfg.add_dir(&dir, "/");
    cfg.print_progress();

    let mut indexbuf = Vec::new();
    v1::scan(&cfg, &mut indexbuf)
        .map_err(|e| error!("Error scanning {:?}: {}", dir, e))?;

    let pool = CpuPool::new(gopt.threads);

    let image_id = get_hash(&mut io::Cursor::new(&indexbuf))
        .expect("hash valid in just created index")
        .into();
    let (blocks, block_size) = {
        let ref mut cur = io::Cursor::new(&indexbuf);
        let mut parser = v1::Parser::new(cur)
            .expect("just created index is valid");
        let header = parser.get_header();
        let block_size = header.get_block_size();
        let mut blocks = HashMap::new();
        for entry in parser.iter() {
            match entry.expect("just created index is valid") {
                v1::Entry::File { ref path, ref hashes, .. } => {
                    let path = Arc::new(path.to_path_buf());
                    for (idx, hash) in hashes.iter().enumerate() {
                        blocks.insert(Hash::new(hash), BlockPointer {
                            file: path.clone(),
                            offset: idx as u64 * block_size,
                        });
                    }
                }
                _ => {}
            }
        }
        (blocks, block_size)
    };
    let image_info = Arc::new(ImageInfo {
        image_id: image_id,
        block_size: block_size,
        index_data: indexbuf,
        location: dir.to_path_buf(),
        blocks: blocks,
    });
    let timestamp = SystemTime::now();
    let mut signatures = HashMap::new();
    for turl in &opt.target_urls {
        if signatures.contains_key(&turl.path[..]) {
            continue;
        }
        signatures.insert(&turl.path[..], sign(SigData {
            path: &turl.path,
            image: image_info.image_id.as_ref(),
            timestamp: to_ms(timestamp),
        }, &opt.private_keys));
    }
    let signatures = Arc::new(signatures);

    tk_easyloop::run(|| {
        let resolver = name::resolver();
        join_all(
            opt.target_urls.iter()
            .map(move |turl| {
                let host = Arc::new(
                    format!("{}:{}", turl.host, gopt.destination_port));
                let host2 = host.clone();
                let host3 = host.clone();
                let host4 = host.clone();
                let image_info = image_info.clone();
                let pool = pool.clone();
                let signatures = signatures.clone();
                resolver.resolve(&host)
                .map_err(move |e| {
                    error!("Error resolving host {}: {}", host2, e);
                })
                .and_then(move |addr| name::pick_hosts(&*host3, addr))
                .and_then(move |names| {
                    join_all(
                        names.iter()
                        .map(move |&addr| {
                            let turl = turl.clone();
                            let signatures = signatures.clone();
                            let host = host4.clone();
                            let image_info = image_info.clone();
                            let pool = pool.clone();
                            Client::spawn(addr, &host, &pool)
                            .and_then(move |mut cli| {
                                info!("Connected to {}", addr);
                                cli.register_index(&image_info);
                                cli.request(AppendDir {
                                    image: image_info.image_id.clone(),
                                    timestamp: timestamp,
                                    signatures: signatures
                                        .get(&turl.path[..]).unwrap()
                                        .clone(),
                                    path: PathBuf::from(turl.path),
                                })
                                .map_err(|e| error!("Request error: {}", e))
                            })
                            .and_then(move |response| {
                                info!("Response from {}: {:?}",
                                    addr, response);
                                // TODO(tailhook) read notifications
                                if !response.accepted {
                                    error!("AppendDir rejected by {}", host);
                                    Either::A(ok(false))
                                } else {
                                    // TODO(tailhook) not just timeout
                                    Either::B(timeout(Duration::new(10, 0))
                                        .map_err(|_| unimplemented!())
                                        .map(|_| true))
                                }
                            })
                            .then(move |res| match res {
                                Ok(x) => Ok(x),
                                Err(()) => {
                                    // Always succeed, for now, so join will
                                    // drive all the futures, even if one
                                    // failed
                                    error!("Address {} has failed.", addr);
                                    Ok(false)
                                }
                            })
                        })
                        .collect::<Vec<_>>() // to unrefer "names"
                    )
                    .map(|results: Vec<bool>| results.iter().any(|x| *x))
                })
            })
            .collect::<Vec<_>>()
        )
        .map(|results:Vec<bool>| results.iter().all(|x| *x))
    })
}


pub fn cli(mut gopt: GlobalOptions, mut args: Vec<String>) -> ! {
    let mut opt = options::UploadOptions::new();
    {
        let mut ap = ArgumentParser::new();
        opt.define(&mut ap);
        gopt.define(&mut ap);
        args.insert(0, String::from("ciruela upload"));
        match ap.parse(args, &mut stdout(), &mut stderr()) {
            Ok(()) => {}
            Err(code) => exit(code),
        }
    }
    let opt = match opt.finalize() {
        Ok(opt) => opt,
        Err(code) => exit(code),
    };
    match do_upload(gopt, opt) {
        Ok(true) => exit(0),
        Ok(false) => exit(1),
        Err(()) => exit(2),
    }
}
