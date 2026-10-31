//! The text form of the IR printed by `dat ir`, and snapshot-tested.

use crate::*;
use std::fmt::Write as _;

pub fn dump(p: &Program) -> String {
    let d = D { types: &p.types };
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
        let params: Vec<String> = f.locals[..f.params as usize]
            .iter()
            .enumerate()
            .map(|(i, l)| format!("%{i} {}{}: {}", if l.by_ref { "mut " } else { "" }, l.name, d.ty(l.ty)))
            .collect();
        let _ = writeln!(
            out,
            "fn{i} {:?}({}) -> {}{} @{}:{}",
            f.name,
            params.join(", "),
            d.ty(f.ret),
            if f.recursive { " recursive" } else { "" },
            f.loc.line,
            f.loc.col
        );
        let first_default = f.params as usize - f.defaults.len();
        for (j, e) in f.defaults.iter().enumerate() {
            let _ = writeln!(out, "  default %{} = {}", first_default + j, d.expr(e));
        }
        for (j, l) in f.locals.iter().enumerate().skip(f.params as usize) {
            let capture = if f.captures.contains(&LocalId(j as u32)) { " capture" } else { "" };
            let by_ref = if l.by_ref { " ref" } else { "" };
            let _ = writeln!(out, "  local %{j} {}: {}{}{capture}{by_ref}", l.name, d.ty(l.ty), if l.reassigned { " mut" } else { "" });
        }
        d.block(&mut out, &f.body, 1);
    }
    out
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

struct D<'a> {
    types: &'a Types,
}

impl D<'_> {
    fn ty(&self, t: Type) -> String {
        self.types.name(t)
    }

    fn block(&self, out: &mut String, b: &Block, depth: usize) {
        for s in b {
            self.stmt(out, s, depth);
        }
    }

    fn stmt(&self, out: &mut String, s: &Stmt, depth: usize) {
        indent(out, depth);
        match s {
            Stmt::Let(l, e) => {
                let _ = writeln!(out, "let %{} = {}", l.0, self.expr(e));
            }
            Stmt::Set(l, e, _) => {
                let _ = writeln!(out, "set %{} = {}", l.0, self.expr(e));
            }
            Stmt::Expr(e) => {
                let _ = writeln!(out, "{}", self.expr(e));
            }
            Stmt::If(c, t, e) => {
                let _ = writeln!(out, "if {}", self.expr(c));
                self.block(out, t, depth + 1);
                if !e.is_empty() {
                    indent(out, depth);
                    out.push_str("else\n");
                    self.block(out, e, depth + 1);
                }
            }
            Stmt::Loop { cond, body, step } => {
                match cond {
                    Some(c) => {
                        let _ = writeln!(out, "loop while {}", self.expr(c));
                    }
                    None => out.push_str("loop\n"),
                }
                self.block(out, body, depth + 1);
                if !step.is_empty() {
                    indent(out, depth);
                    out.push_str("step\n");
                    self.block(out, step, depth + 1);
                }
            }
            Stmt::Break => out.push_str("break\n"),
            Stmt::Continue => out.push_str("continue\n"),
            Stmt::Return(None) => out.push_str("return\n"),
            Stmt::Return(Some(e)) => {
                let _ = writeln!(out, "return {}", self.expr(e));
            }
            Stmt::Throw(e, loc) => {
                let _ = writeln!(out, "throw @{}:{} {}", loc.line, loc.col, self.expr(e));
            }
            Stmt::Try { body, catches } => {
                out.push_str("try\n");
                self.block(out, body, depth + 1);
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
                    self.block(out, &c.body, depth + 1);
                }
            }
            Stmt::Store(place, e) => {
                let _ = writeln!(out, "store {} = {}", self.place(place), self.expr(e));
            }
            Stmt::ForOf { var, iter, body } => {
                let what = match iter {
                    Iter::Range { start, end, step, loc } => {
                        format!("(range {} {} {} {})", at(loc), self.expr(start), self.expr(end), self.expr(step))
                    }
                    Iter::Each(e) => self.expr(e),
                };
                let _ = writeln!(out, "for %{} of {what}", var.0);
                self.block(out, body, depth + 1);
            }
            Stmt::ForMut { var, list, body, .. } => {
                let _ = writeln!(out, "for mut %{} of {}", var.0, self.place(list));
                self.block(out, body, depth + 1);
            }
        }
    }

    fn place(&self, p: &Place) -> String {
        let mut s = format!("%{}", p.root.0);
        for step in &p.steps {
            match step {
                Step::Index(i, loc) => {
                    let _ = write!(s, "[{} {}]", at(loc), self.expr(i));
                }
                Step::Key(k, loc) => {
                    let _ = write!(s, "{{{} {}}}", at(loc), self.expr(k));
                }
                Step::Pos(n) => {
                    let _ = write!(s, ".{n}");
                }
            }
        }
        s
    }

    fn elems(&self, elems: &[Elem]) -> String {
        let parts: Vec<String> = elems
            .iter()
            .map(|e| match e {
                Elem::One(x) => self.expr(x),
                Elem::Spread(x) => format!("...{}", self.expr(x)),
            })
            .collect();
        parts.join(" ")
    }

    fn expr(&self, e: &Expr) -> String {
        match &e.kind {
            ExprKind::Int(n) => n.to_string(),
            ExprKind::Float(x) => format!("{x:?}"),
            ExprKind::Bool(b) => b.to_string(),
            ExprKind::Str(s) => format!("{s:?}"),
            ExprKind::Local(l) => format!("%{}", l.0),
            ExprKind::Move(l) => format!("(move %{})", l.0),
            ExprKind::Call { func, args, loc } => {
                let args: Vec<String> = args.iter().map(|a| self.expr(a)).collect();
                format!("(call fn{} {} {})", func.0, at(loc), args.join(" ")).replace(" )", ")")
            }
            ExprKind::IntArith { op, lhs, rhs, loc } => {
                let name = match op {
                    IntOp::Add => "add",
                    IntOp::Sub => "sub",
                    IntOp::Mul => "mul",
                    IntOp::Rem => "rem",
                };
                format!("({name}.int {} {} {})", at(loc), self.expr(lhs), self.expr(rhs))
            }
            ExprKind::IntNeg { arg, loc } => format!("(neg.int {} {})", at(loc), self.expr(arg)),
            ExprKind::FloatArith { op, lhs, rhs } => {
                let name = match op {
                    FloatOp::Add => "add",
                    FloatOp::Sub => "sub",
                    FloatOp::Mul => "mul",
                    FloatOp::Div => "div",
                    FloatOp::Rem => "rem",
                };
                format!("({name}.float {} {})", self.expr(lhs), self.expr(rhs))
            }
            ExprKind::FloatNeg(a) => format!("(neg.float {})", self.expr(a)),
            ExprKind::Builtin { op, args, loc } => {
                let args: Vec<String> = args.iter().map(|a| self.expr(a)).collect();
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
                format!("({name}.{} {} {})", self.ty(lhs.ty), self.expr(lhs), self.expr(rhs))
            }
            ExprKind::Not(a) => format!("(not {})", self.expr(a)),
            ExprKind::And(a, b) => format!("(and {} {})", self.expr(a), self.expr(b)),
            ExprKind::Or(a, b) => format!("(or {} {})", self.expr(a), self.expr(b)),
            ExprKind::Cond { cond, then, els } => format!("(cond {} {} {})", self.expr(cond), self.expr(then), self.expr(els)),
            ExprKind::Concat(parts) => {
                let parts: Vec<String> = parts.iter().map(|a| self.expr(a)).collect();
                format!("(concat {})", parts.join(" "))
            }
            ExprKind::ToStr(a) => format!("(str.{} {})", self.ty(a.ty), self.expr(a)),
            ExprKind::NewError { kind, message } => format!("(new {} {})", kind.name(), self.expr(message)),
            ExprKind::ErrorMessage(a) => format!("(message {})", self.expr(a)),
            ExprKind::Print(a) => format!("(print {})", self.expr(a)),
            ExprKind::Assert(a) => {
                let cond = match &a.cond {
                    AssertCond::Plain(c) => self.expr(c),
                    AssertCond::Compare { op, lhs, rhs } => format!("({} {} {})", op.symbol(), self.expr(lhs), self.expr(rhs)),
                };
                let msg = a.message.as_ref().map(|m| format!(" {}", self.expr(m))).unwrap_or_default();
                format!("(assert {} {cond}{msg})", at(&a.loc))
            }
            ExprKind::Fail { message, loc } => format!("(fail {} {})", at(loc), self.expr(message)),
            ExprKind::Diverge(a) => format!("(diverge.{} {})", self.ty(e.ty), self.expr(a)),
            ExprKind::Ref(p) => format!("(mut {})", self.place(p)),
            ExprKind::FuncRef(f) => format!("fn{}", f.0),
            ExprKind::Closure { func, captures } => {
                let caps: Vec<String> = captures.iter().map(|l| format!(" %{}", l.0)).collect();
                format!("(closure fn{}{})", func.0, caps.concat())
            }
            ExprKind::CallValue { callee, args, loc } => {
                let args: Vec<String> = args.iter().map(|a| format!(" {}", self.expr(a))).collect();
                format!("(call-value {} {}{})", at(loc), self.expr(callee), args.concat())
            }
            ExprKind::ListLit(elems) => format!("(list.{} {})", self.ty(e.ty), self.elems(elems)).replace(" )", ")"),
            ExprKind::SetLit(elems) => format!("(set.{} {})", self.ty(e.ty), self.elems(elems)).replace(" )", ")"),
            ExprKind::MapLit(entries) => {
                let parts: Vec<String> = entries
                    .iter()
                    .map(|en| match en {
                        Entry::Pair(k, v) => format!(" ({} {})", self.expr(k), self.expr(v)),
                        Entry::Spread(x) => format!(" ...{}", self.expr(x)),
                    })
                    .collect();
                format!("(map.{}{})", self.ty(e.ty), parts.concat())
            }
            ExprKind::TupleLit(items) => {
                let items: Vec<String> = items.iter().map(|a| format!(" {}", self.expr(a))).collect();
                format!("(tuple{})", items.concat())
            }
            ExprKind::Index { base, index, loc } => format!("(index {} {} {})", at(loc), self.expr(base), self.expr(index)),
            ExprKind::TupleGet(base, n) => format!("(get.{n} {})", self.expr(base)),
            ExprKind::Method { op, args, loc } => {
                let args: Vec<String> = args.iter().map(|a| format!(" {}", self.expr(a))).collect();
                let at = if op.throws() || op.takes_callback() { format!(" {}", at(loc)) } else { String::new() };
                format!("({}{at}{})", op.name(), args.concat())
            }
            ExprKind::Mutate { op, place, args, loc } => {
                let args: Vec<String> = args.iter().map(|a| format!(" {}", self.expr(a))).collect();
                let at = if op.throws() || op.takes_callback() { format!(" {}", at(loc)) } else { String::new() };
                format!("({}{at} {}{})", op.name(), self.place(place), args.concat())
            }
        }
    }
}

fn at(loc: &Loc) -> String {
    format!("@{}:{}", loc.line, loc.col)
}
