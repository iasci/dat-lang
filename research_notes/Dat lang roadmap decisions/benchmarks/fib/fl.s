	.build_version macos, 11, 0
	.section	__TEXT,__text,regular,pure_instructions
	.globl	__RNvCseFTUA9E9Vs9_2fl3fib
	.p2align	2
__RNvCseFTUA9E9Vs9_2fl3fib:
	.cfi_startproc
	stp	x20, x19, [sp, #-32]!
	.cfi_def_cfa_offset 32
	stp	x29, x30, [sp, #16]
	add	x29, sp, #16
	.cfi_def_cfa w29, 16
	.cfi_offset w30, -8
	.cfi_offset w29, -16
	.cfi_offset w19, -24
	.cfi_offset w20, -32
	.cfi_remember_state
	subs	x19, x0, #2
	b.lt	LBB0_2
	sub	x0, x0, #1
	bl	__RNvCseFTUA9E9Vs9_2fl3fib
	mov	x20, x0
	mov	x0, x19
	bl	__RNvCseFTUA9E9Vs9_2fl3fib
	adds	x0, x20, x0
	b.vs	LBB0_3
LBB0_2:
	.cfi_def_cfa wsp, 32
	ldp	x29, x30, [sp, #16]
	ldp	x20, x19, [sp], #32
	.cfi_def_cfa_offset 0
	.cfi_restore w30
	.cfi_restore w29
	.cfi_restore w19
	.cfi_restore w20
	ret
LBB0_3:
	.cfi_restore_state
Lloh0:
	adrp	x0, l_anon.400f9d6ab7f8b92e76ae96f4e0cffa33.1@PAGE
Lloh1:
	add	x0, x0, l_anon.400f9d6ab7f8b92e76ae96f4e0cffa33.1@PAGEOFF
	bl	__RNvNtNtCsjIpoRzUjyHQ_4core9panicking11panic_const24panic_const_add_overflow
	.loh AdrpAdd	Lloh0, Lloh1
	.cfi_endproc

	.section	__TEXT,__cstring,cstring_literals
l_anon.400f9d6ab7f8b92e76ae96f4e0cffa33.0:
	.asciz	"fl.rs"

	.section	__DATA,__const
	.p2align	3, 0x0
l_anon.400f9d6ab7f8b92e76ae96f4e0cffa33.1:
	.quad	l_anon.400f9d6ab7f8b92e76ae96f4e0cffa33.0
	.asciz	"\005\000\000\000\000\000\000\000\001\000\000\0003\000\000"

.subsections_via_symbols
