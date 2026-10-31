from collections import deque

n = 1_000_000
dist = [-1] * n
dist[0] = 0
queue = deque([0])
reached = 1
while queue:
    v = queue.popleft()
    for w in ((v * 2) % n, (v + 1) % n, (v * 3 + 1) % n):
        if dist[w] == -1:
            dist[w] = dist[v] + 1
            reached += 1
            queue.append(w)
print(reached, dist[n - 1], dist[12345])
