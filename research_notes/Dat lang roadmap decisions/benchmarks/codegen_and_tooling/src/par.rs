use std::time::Instant; use std::fs;
fn main(){
  let corpus=std::env::args().nth(1).unwrap();
  let mut paths:Vec<_>=fs::read_dir(&corpus).unwrap().map(|e|e.unwrap().path()).collect(); paths.sort();
  let nt=std::thread::available_parallelism().unwrap().get();
  for round in 0..3 {
    let t=Instant::now();
    let chunks:Vec<Vec<std::path::PathBuf>>={let mut c=vec![vec![];nt]; for (i,p) in paths.iter().enumerate(){c[i%nt].push(p.clone());} c};
    let hs:Vec<_>=chunks.into_iter().map(|c|std::thread::spawn(move||c.iter().map(|p|fs::read(p).unwrap().len()).sum::<usize>())).collect();
    let s:usize=hs.into_iter().map(|h|h.join().unwrap()).sum();
    let par=t.elapsed().as_secs_f64()*1000.0;
    let t=Instant::now();
    let chunks:Vec<Vec<std::path::PathBuf>>={let mut c=vec![vec![];nt]; for (i,p) in paths.iter().enumerate(){c[i%nt].push(p.clone());} c};
    let hs:Vec<_>=chunks.into_iter().map(|c|std::thread::spawn(move||c.iter().map(|p|fs::metadata(p).unwrap().len()).sum::<u64>())).collect();
    let _s2:u64=hs.into_iter().map(|h|h.join().unwrap()).sum();
    let pst=t.elapsed().as_secs_f64()*1000.0;
    println!("round {round}: parallel read {} files ({} B) on {nt} threads: {:.3} ms; parallel stat: {:.3} ms", paths.len(), s, par, pst);
  }
}
