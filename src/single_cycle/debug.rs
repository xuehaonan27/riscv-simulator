use super::cpu::CPU;
use crate::{
    core::reg::REGNAME,
    error::{Error, Result},
};
use clap::{Parser, Subcommand};
use clap_num::maybe_hex;
use std::io::{self, BufRead, Write};

const REDB_BUF_SIZE: usize = 64;

pub struct REDB<'a> {
    // Command line input buffer
    buf: String,

    // CPU
    cpu: &'a mut CPU<'a>,
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about, disable_help_flag = true)]
struct DebugArgs {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    #[clap(alias = "help")]
    H,
    #[clap(alias = "c")]
    Continue,
    #[clap(alias = "q")]
    Quit,
    #[clap(alias = "si")]
    Step {
        #[clap(default_value_t = 1)]
        n: i32,
    },
    Info {
        r: String,
    },
    #[clap(alias = "x")]
    Scan {
        n: u64,
        #[clap(value_parser=maybe_hex::<u64>)]
        vaddr: u64,
    },
    #[clap(alias = "bt")]
    Backtrace,
}

impl<'a> REDB<'a> {
    pub fn new(cpu: &'a mut CPU<'a>) -> REDB<'a> {
        REDB {
            buf: String::with_capacity(REDB_BUF_SIZE),
            cpu,
        }
    }

    pub fn run(&mut self) {
        loop {
            print!("(REDB)>>> ");
            io::stdout().flush().expect("Fail to flush");
            let cmd = match self.listen() {
                Ok(cmd) => cmd,
                Err(e) => {
                    println!("{}", e);
                    continue;
                }
            };
            if cmd.is_none() {
                continue;
            }
            match cmd.unwrap() {
                Commands::H => print_help_info(),
                Commands::Continue => match self.cpu.cpu_exec(None) {
                    Ok(_) => {
                        println!("REDB: CPU executed to end.");
                        break;
                    }
                    Err(e) => {
                        println!("REDB: CPU raised exception: {}", e);
                        continue;
                    }
                },
                Commands::Quit => {
                    println!("REDB: Exit REDB");
                    break;
                }
                Commands::Step { n } => {
                    if n.is_negative() {
                        println!("REDB: steps cannot be negative");
                    }
                    println!("REDB: execute {n} steps");
                    for i in 1..=n {
                        if let Err(e) = self.cpu.exec_once() {
                            println!("REDB: stopped after executed {i} steps");
                            println!("{e}");
                            break;
                        }
                    }
                    println!("REDB: executed {n} steps");
                }
                Commands::Info { r } => {
                    if r == "r" {
                        for i in 0..32 {
                            let reg_name = format!("x{i}");
                            let reg = self.cpu.reg_val_by_name(&reg_name).unwrap();
                            println!("{} ({}) \t: {}\t{:#x}", reg_name, REGNAME[i], reg, reg);
                        }
                        let pc = self.cpu.pc();
                        println!("{}\t\t: {}\t{:#x}", "pc", pc, pc);
                    } else {
                        match self.cpu.reg_val_by_name(&r) {
                            Ok(reg) => {
                                println!("{}\t: {}\t{:#x}", r, reg, reg);
                            }
                            Err(e) => {
                                println!("REDB: {e}");
                            }
                        }
                    }
                }
                Commands::Scan { n, vaddr } => {
                    for i in 0..n {
                        let p_vaddr = vaddr + 4 * i;
                        let val = self.cpu.mread::<u64>(p_vaddr);
                        println!("{:#x}: {:016x}", p_vaddr, val);
                    }
                }
                Commands::Backtrace => {
                    println!("REDB: backtrace");
                    self.cpu.backtrace()
                }
            }
        }
    }

    // Listen for user's input
    fn listen(&mut self) -> Result<Option<Commands>> {
        self.buf.clear();
        let stdin = io::stdin();
        let mut stdin = stdin.lock();
        stdin.read_line(&mut self.buf)?;

        let buf = self.buf.trim();
        if buf.is_empty() {
            return Ok(None);
        }

        let mut itr: Vec<&str> = self.buf.split_whitespace().collect();
        itr.insert(0, "DebugArgs");
        let dbargs = DebugArgs::try_parse_from(itr).map_err(|e| Error::DbgParse(e.to_string()))?;
        Ok(Some(dbargs.command))
    }
}

fn print_help_info() {
    let help = r#"
REDB: RISC-V Environment DeBugger. 
    Command     Example         Detail
    help        help            Print this help.
    c           c               Execute the program to end.
    q           q               Quit the debugger (also the simulator).
    si [N]      si 10           Step the program for N steps and pause (N default to 1).
    info <reg>  info sp         Print a register's status.
    info r      info r          Print all registers' status (including PC).
    x N ADDR    x 10 0x80000000 Print N quad-words starting at ADDR.
"#;
    println!("{help}")
}
