use std::time::Instant;
use std::process::Command;
use std::fs;
fn fnv(b:&[u8])->u64{let mut h:u64=0xcbf29ce484222325;for &x in b{h^=x as u64;h=h.wrapping_mul(0x100000001b3);}h}
fn lex(b:&[u8])->usize{ // naive tokenizer: idents, numbers, strings, punct
  let mut i=0;let mut n=0;let len=b.len();
  while i<len{let c=b[i];
    if c==b' '||c==b'\n'||c==b'\t'||c==b'\r'{i+=1;continue;}
    if c.is_ascii_alphabetic()||c==b'_'{while i<len&&(b[i].is_ascii_alphanumeric()||b[i]==b'_'){i+=1;}n+=1;continue;}
    if c.is_ascii_digit(){while i<len&&(b[i].is_ascii_digit()||b[i]==b'.'){i+=1;}n+=1;continue;}
    if c==b'"'{i+=1;while i<len&&b[i]!=b'"'{i+=1;}i+=1;n+=1;continue;}
    if c==b'/'&&i+1<len&&b[i+1]==b'/'{while i<len&&b[i]!=b'\n'{i+=1;}continue;}
    i+=1;n+=1;}
  n}
fn main(){
  let args:Vec<String>=std::env::args().collect();
  let noop=&args[1]; let corpus=&args[2];
  // 1. process spawn
  for _ in 0..20{Command::new(noop).status().unwrap();}
  let t=Instant::now(); let k=300;
  for _ in 0..k{Command::new(noop).status().unwrap();}
  println!("spawn+wait noop binary: {:.3} ms avg", t.elapsed().as_secs_f64()*1000.0/k as f64);
  // 2. stat all files
  let mut paths:Vec<_>=fs::read_dir(corpus).unwrap().map(|e|e.unwrap().path()).collect(); paths.sort();
  let t=Instant::now(); let mut m=0u64;
  for p in &paths{let md=fs::metadata(p).unwrap(); m^=md.len();}
  println!("stat {} files: {:.3} ms", paths.len(), t.elapsed().as_secs_f64()*1000.0);
  // 3. read all files
  let t=Instant::now(); let mut total=0usize; let mut bufs=vec![];
  for p in &paths{let b=fs::read(p).unwrap(); total+=b.len(); bufs.push(b);}
  println!("read {} files ({} bytes): {:.3} ms", paths.len(), total, t.elapsed().as_secs_f64()*1000.0);
  // 4. hash all
  let t=Instant::now(); let mut h=0u64; for b in &bufs{h^=fnv(b);}
  println!("FNV-1a hash all: {:.3} ms ({})", t.elapsed().as_secs_f64()*1000.0, h&1);
  // 5. naive lex single thread
  let t=Instant::now(); let mut toks=0; for b in &bufs{toks+=lex(b);}
  let lx=t.elapsed().as_secs_f64()*1000.0;
  println!("naive lex single-thread: {} tokens in {:.3} ms", toks, lx);
  // 6. naive lex with std threads (14 chunks)
  let t=Instant::now();
  let nt=std::thread::available_parallelism().unwrap().get();
  let chunks:Vec<Vec<Vec<u8>>>= {let mut c=vec![vec![];nt]; for (i,b) in bufs.iter().enumerate(){c[i%nt].push(b.clone());} c};
  let t2=Instant::now();
  let hs:Vec<_>=chunks.into_iter().map(|c|std::thread::spawn(move||c.iter().map(|b|lex(b)).sum::<usize>())).collect();
  let s:usize=hs.into_iter().map(|h|h.join().unwrap()).sum();
  println!("naive lex {} threads: {} tokens in {:.3} ms (incl thread spawn; {:.3} ms incl clone)", nt, s, t2.elapsed().as_secs_f64()*1000.0, t.elapsed().as_secs_f64()*1000.0);
  // 7. write+read a 20MB blob (cache file proxy)
  let blob=vec![7u8;20*1024*1024]; let cp=format!("{}/../cache.bin",corpus);
  fs::write(&cp,&blob).unwrap();
  let t=Instant::now(); let r=fs::read(&cp).unwrap();
  println!("read 20MB cache file (warm page cache): {:.3} ms ({})", t.elapsed().as_secs_f64()*1000.0, r.len());
  let _=m;
}
