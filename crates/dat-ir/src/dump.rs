//! The text form of the IR printed by `dat ir`, and snapshot-tested.

use crate::*;
use std::fmt::Write as _;

pub fn dump(p: &Program) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "program {:?}", p.file);
    if let Some(main) = p.main {
        let _ = writeln!(out, "main = fn{}", main.0);
    }
    for t in &p.tests {
        let _ = writeln!(out, "test {:?} = fn{} @{}:{}", t.name, t.func.0, t.loc.line, t.loc.col);
    }
    for (i, f) in p.functions.iter().enumerate() {
        out.push('\n');
        let params: Vec<String> =
            f.locals[..f.params as usize].iter().enumerate().map(|(i, l)| format!("%{i} {}: {}", l.name, l.ty.name())).collect();
        let _ = writeln!(
            out,
            "fn{i} {:?}({}) -> {}{} @{}:{}",
            f.name,
            params.join(", "),
            f.ret.name(),
            if f.recursive { " recursive" } else { "" },
            f.loc.line,
            f.loc.col
        );
        let first_default = f.params as usize - f.defaults.len();
        for (j, d) in f.defaults.iter().enumerate() {
            let _ = writeln!(out, "  default %{} = {}", first_default + j, expr(d));
        }
        for (j, l) in f.locals.iter().enumerate().skip(f.params as usize) {
            let _ = writeln!(out, "  local %{j} {}: {}{}", l.name, l.ty.name(), if l.reassigned { " mut" } else { "" });
        }
        block(&mut out, &f.body, 1);
    }
    out
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn block(out: &mut String, b: &Block, depth: usize) {
    for s in b {
        stmt(out, s, depth);
    }
}

fn stmt(out: &mut String, s: &Stmt, depth: usize) {
    indent(out, depth);
    match s {
        Stmt::Let(l, e) => {
            let _ = writeln!(out, "let %{} = {}", l.0, expr(e));
        }
        Stmt::Set(l, e) => {
            let _ = writeln!(out, "set %{} = {}", l.0, expr(e));
        }
        Stmt::Expr(e) => {
            let _ = writeln!(out, "{}", expr(e));
        }
        Stmt::If(c, t, e) => {
            let _ = writeln!(out, "if {}", expr(c));
            block(out, t, depth + 1);
            if !e.is_empty() {
                indent(out, depth);
                out.push_str("else\n");
                block(out, e, depth + 1);
            }
        }
        Stmt::Loop { cond, body, step } => {
            match cond {
                Some(c) => {
                    let _ = writeln!(out, "loop while {}", expr(c));
                }
                None => out.push_str("loop\n"),
            }
            block(out, body, depth + 1);
            if !step.is_empty() {
                indent(out, depth);
                out.push_str("step\n");
                block(out, step, depth + 1);
            }
        }
        Stmt::Break => out.push_str("break\n"),
        Stmt::Continue => out.push_str("continue\n"),
        Stmt::Return(None) => out.push_str("return\n"),
        Stmt::Return(Some(e)) => {
            let _ = writeln!(out, "return {}", expr(e));
        }
        Stmt::Throw(e, loc) => {
            let _ = writeln!(out, "throw @{}:{} {}", loc.line, loc.col, expr(e));
        }
        Stmt::Try { body, catches } => {
            out.push_str("try\n");
            block(out, body, depth + 1);
            for c in catches {
                indent(out, depth);
                let filter = match c.filter {
                    CatchFilter::Is(t) => t.name().to_string(),
                    CatchFilter::All => "*".to_string(),
                    CatchFilter::AllAsError => "* as Error".to_string(),
                };
                match c.bind {
                    Some(l) => {
                        let _ = writeln!(out, "catch {filter} -> %{}", l.0);
                    }
                    None => {
                        let _ = writeln!(out, "catch {filter}");
                    }
                }
                block(out, &c.body, depth + 1);
            }
        }
    }
}

fn at(loc: &Loc) -> String {
    format!("@{}:{}", loc.line, loc.col)
}

fn expr(e: &Expr) -> String {
    match &e.kind {
        ExprKind::Int(n) => n.to_string(),
        ExprKind::Float(x) => format!("{x:?}"),
        ExprKind::Bool(b) => b.to_string(),
        ExprKind::Str(s) => format!("{s:?}"),
        ExprKind::Local(l) => format!("%{}", l.0),
        ExprKind::Call { func, args, loc } => {
            let args: Vec<String> = args.iter().map(expr).collect();
            format!("(call fn{} {} {})", func.0, at(loc), args.join(" ")).replace(" )", ")")
        }
        ExprKind::IntArith { op, lhs, rhs, loc } => {
            let name = match op {
                IntOp::Add => "add",
                IntOp::Sub => "sub",
                IntOp::Mul => "mul",
                IntOp::Rem => "rem",
            };
            format!("({name}.int {} {} {})", at(loc), expr(lhs), expr(rhs))
        }
        ExprKind::IntNeg { arg, loc } => format!("(neg.int {} {})", at(loc), expr(arg)),
        ExprKind::FloatArith { op, lhs, rhs } => {
            let name = match op {
                FloatOp::Add => "add",
                FloatOp::Sub => "sub",
                FloatOp::Mul => "mul",
                FloatOp::Div => "div",
                FloatOp::Rem => "rem",
            };
            format!("({name}.float {} {})", expr(lhs), expr(rhs))
        }
        ExprKind::FloatNeg(a) => format!("(neg.float {})", expr(a)),
        ExprKind::Builtin { op, args, loc } => {
            let args: Vec<String> = args.iter().map(expr).collect();
            let at = if op.throws() { format!(" {}", at(loc)) } else { String::new() };
            format!("({}.{}{at} {})", op.rt_module(), op.rt_name(), args.join(" "))
        }
        ExprKind::Compare { op, lhs, rhs } => {
            let name = match op {
                CmpOp::Lt => "lt",
                CmpOp::Le => "le",
                CmpOp::Gt => "gt",
                CmpOp::Ge => "ge",
                CmpOp::Eq => "eq",
                CmpOp::Ne => "ne",
            };
            format!("({name}.{} {} {})", lhs.ty.name(), expr(lhs), expr(rhs))
        }
        ExprKind::Not(a) => format!("(not {})", expr(a)),
        ExprKind::And(a, b) => format!("(and {} {})", expr(a), expr(b)),
        ExprKind::Or(a, b) => format!("(or {} {})", expr(a), expr(b)),
        ExprKind::Cond { cond, then, els } => format!("(cond {} {} {})", expr(cond), expr(then), expr(els)),
        ExprKind::Concat(parts) => {
            let parts: Vec<String> = parts.iter().map(expr).collect();
            format!("(concat {})", parts.join(" "))
        }
        ExprKind::ToStr(a) => format!("(str.{} {})", a.ty.name(), expr(a)),
        ExprKind::NewError { kind, message } => format!("(new {} {})", kind.name(), expr(message)),
        ExprKind::ErrorMessage(a) => format!("(message {})", expr(a)),
        ExprKind::Print(a) => format!("(print {})", expr(a)),
        ExprKind::Assert(a) => {
            let cond = match &a.cond {
                AssertCond::Plain(c) => expr(c),
                AssertCond::Compare { op, lhs, rhs } => format!("({} {} {})", op.symbol(), expr(lhs), expr(rhs)),
            };
            let msg = a.message.as_ref().map(|m| format!(" {}", expr(m))).unwrap_or_default();
            format!("(assert {} {cond}{msg})", at(&a.loc))
        }
        ExprKind::Fail { message, loc } => format!("(fail {} {})", at(loc), expr(message)),
        ExprKind::Diverge(a) => format!("(diverge.{} {})", e.ty.name(), expr(a)),
    }
}
