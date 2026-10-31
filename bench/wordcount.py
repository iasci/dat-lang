counts = {}
for i in range(500_000):
    word = f"w{(i * 7919) % 5000}"
    counts[word] = counts.get(word, 0) + 1
print(len(counts), counts["w0"], counts["w4999"])
