# Money and decimals design

dat should be a safe choice for financial systems: amounts that are always exact, rounding that only happens where the code asks for it, and no mixing of currencies. This note sets out how, without making the rest of the language pay for it. The owner has made all five decisions it asks for. Nothing here is built yet.

## The problems

- **Ordinary decimal numbers aren't exact.** A `float` stores numbers in binary, and in binary `0.10` can't be written down exactly, just as 1/3 can't be written in decimal. So `0.1 + 0.2` is `0.30000000000000004`. Measured here: adding up 10 million order lines (`total += price * quantity`) in floats gave 1,915,919,207.7999995 instead of 1,915,919,207.80.
- **Rounding has to be on purpose.** 19% tax on 9.99 is 1.8981, which must become 1.90 at some point. Laws and contracts prescribe how exact halves round: EU rules for converting currencies to euros round them up, while the rule called banker's rounding sends them to the even digit. The language must never round money on its own.
- **Amounts belong to a currency.** 10 euros plus 10 dollars isn't 20 of anything.
- **Splitting mustn't lose cents.** 100.00 split three ways is 33.333…, and three shares of 33.33 add up to 99.99. One share has to get the extra cent.

## What dat already guarantees

- `int` overflow throws a `RangeError` instead of wrapping around to a negative number.
- `/` never drops a remainder silently: two ints give a `float`, and turning it back into an `int` is explicit.
- Values are copies, so no other part of the program can change an amount someone holds.
- Every runtime error can be caught, so one bad amount doesn't have to stop a payment run.

Amounts kept as whole cents in an `int`, as Stripe's API does, are exact today. A decimal type removes the need to convert to and from cents by hand, and handles amounts with more digits, such as a price of 0.0045 per message.

## Decision 1: what `19.99` means

Decided by the owner: a `float`, unless the code asks for a decimal through the type.

`0.1` is a float, and dividing two ints gives a float, as §03 says. Nothing in everyday code changes or gets slower. Where a value is declared a decimal, the numbers written next to it are read exactly, the way `const ratio: float = 1` already makes the `1` a float:

```
const price: decimal = 19.99;   // exact, because the type says decimal
const total = price * 3;        // decimal: 59.97
const gross = price * 1.19;     // decimal: 1.19 is read exactly, because price is a decimal
const ratio = 0.5;              // float
const bad = price * ratio;      // ❌ a decimal and a float don't mix silently
                                //    fixes: price.toFloat() * ratio, or price * ratio.toDecimal()
```

C# needs a marker on every exact number (`19.99m`), and Java and Python build them from text (`new BigDecimal("19.99")`, `Decimal("19.99")`). Forgetting the marker gives no error, only a wrong cent much later, and most money bugs in those languages come from it. In dat, once an amount is a decimal, every number written next to it is exact without a marker: a struct field, a parameter or a value read from a database. A float can't slip in without an error.

**Not chosen:** making `19.99` exact everywhere, as Groovy, Raku and PostgreSQL do. It puts decimals at the heart of the language: every average and every division of two ints would become a decimal, and ordinary code would get slower to protect money code. In the order-line loop above, a 128-bit decimal took 9.3–9.7 ns per line against 3.8–4.0 ns for floats, about 2.4×.

## Decision 2: how decimals behave

Decided by the owner: a fixed size of 28 significant digits, `/` that keeps 28 digits, and equality by value.

- **Size.** A decimal is 128 bits and holds at least 28 significant digits, however many come after the point. It needs no memory allocation and copies as cheaply as an `int`. 28 digits is enough for the world's yearly output in cents, with ten more decimals to spare.
- **Adding, subtracting and multiplying are exact.** A result that needs more than 28 digits throws a `RangeError`, as `int` overflow does. A literal with more digits than fit is a compile error.
- **Division keeps 28 digits.** `19.99 / 1.19` is `16.79831932773109243697478992`: the true answer goes on forever, so the last digit is rounded to the nearest. That's the only rounding the code doesn't ask for, and for amounts it sits about 25 digits below a cent. C#'s decimal type works the same way. Dividing by zero throws a `RangeError`, since decimals have no `Infinity`.
- **An `int` mixes with a decimal**, because both are exact: `price * 3` is a decimal.
- **Equal by value.** `19.90 == 19.9` is true; Java's `BigDecimal` says they're different, a well-known source of bugs. A decimal can be a set element and a map key, which a float can't.

Proposed, and not objected to:

- **Printed as written.** `19.90` prints as `19.90`, because an invoice that says 19.90 should keep saying it. A calculated result keeps the places of what went into it: `19.90 + 0.1` prints `20.00`, and `1.50 * 2` prints `3.00`.
- **Conversions are explicit.** `"19.99".toDecimal()` is exact and throws a `ValueError` for text that isn't a number. `x.toDecimal()` on a float takes the number as it prints, as Java's `BigDecimal.valueOf` does, so `0.1.toDecimal()` is exactly `0.1`. Python's `Decimal(0.1)` shows the other way: it gives `0.1000000000000000055511151231257827021181583404541015625`, the float's actual value. `d.toFloat()` goes back.

## Decision 3: where decimals come from

Decided by the owner: `decimal` is a built-in type, written like `int` and `float`, with no import. `Rounding` comes from `"@dat/math"`, as `parallel` is built in while `OnFailure` comes from `"@dat/tasks"`. `Money` and `Currency` come from `"@dat/money"`.

The compiler has to know decimals anyway, to read the numbers next to one exactly, so a module would only add an import line. Being built in doesn't put decimals at the heart of the language: nothing is a decimal unless the code says so. `decimal` isn't a reserved word; like the other type names, it's only recognized in types.

**Not chosen:** `Decimal` from a module, `"@dat/decimal"`.

## Decision 4: rounding

Decided by the owner: every rounding names its rule.

```
const tax = (net * 0.19).round(2, .HalfUp);
```

`.HalfUp` is short for `Rounding.HalfUp`; the parameter's type says which enum it belongs to, so rounding needs no import ([§18](../reference/18-enums-and-unions.md#leaving-out-the-enums-name)).

| Rule | 0.125 to two places | -0.125 to two places |
| --- | --- | --- |
| `HalfUp`: halves away from zero | 0.13 | -0.13 |
| `HalfEven`: halves to the even digit | 0.12 | -0.12 |
| `Up`: away from zero | 0.13 | -0.13 |
| `Down`: toward zero | 0.12 | -0.12 |
| `Ceiling`: toward positive | 0.13 | -0.12 |
| `Floor`: toward negative | 0.12 | -0.13 |

There's no default rule, because the right one depends on the law or the contract, and a default hides that choice. Wrong rounding usually shows up only in an audit. Leaving the rule out is an error whose fix lists the rules.

**Not chosen:** `HalfUp` as the default, as taught in school.

Going back and forth doesn't give the original amount, in any number type: 19.99 / 1.19 rounds to 16.80, and 16.80 × 1.19 is 19.992. Invoicing code works out the tax once and derives the other figure from it (net = gross − tax), so the two always add up. The money module can offer that as one call.

## Decision 5: money

Decided by the owner: a standard library module, `"@dat/money"`, with a `Money` value that's an amount and a currency:

```
import { Money } from "@dat/money";

const price = Money.of(19.99, .EUR);
const total = price * 3;                                  // 59.97 EUR
const shares = Money.of(100, .EUR).split(3);             // [33.34 EUR, 33.33 EUR, 33.33 EUR]
const mixed = price + Money.of(5, .USD);                 // throws CurrencyError
```

- `Currency` lists the ISO 4217 currencies, each with its number of decimal places: 2 for EUR, 0 for JPY, 3 for KWD.
- Amounts can have more places than the currency, for unit prices such as 0.0045 EUR per message. `m.round(.HalfUp)` rounds to the currency's places.
- Adding, subtracting or comparing two currencies throws a `CurrencyError`. The compiler can't catch it in general, because a currency is often only known at run time, such as one read from a database.
- `split(n)` and `allocate([70, 30])` hand out the leftover cents one at a time, so the shares always add up to the whole.
- Money times a number is money; money times money is an error.
- Converting takes a rate and a rule: `m.convert(.USD, rate, .HalfEven)`. Where rates come from is the program's business.
- `toString()` gives `19.99 EUR`. Formatting for a country, such as `19,99 €`, comes with the rest of text formatting.

## What the code needs once these are decided

- A spec section for decimals, rounding and money, and §01's literal rules extended to decimals.
- `dat_rt::Decimal`: 128 bits, with exact `+`, `-` and `*`, division to 28 digits, `round` and the conversions, used by both modes. An outside library qualifies only under the rule in `AGENTS.md`; Rust's `rust_decimal` is the candidate, at 28 digits.
- `Rounding` built into the standard library before user-defined enums exist.
- `Money` needs structs, enums and operators on your own types, so it comes after M3.

## When

Floats no longer wait for this: `19.99` is a float either way, so M1 builds floats as planned. Decimals can come any time after M1, and money after M3.
