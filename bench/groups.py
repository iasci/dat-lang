by_team = {}
for i in range(1_000_000):
    team = (i * 31) % 1000
    members = by_team.get(team, [])
    members.append(i)
    by_team[team] = members
total = 0
for team, members in by_team.items():
    total += len(members)
print(len(by_team), total, len(by_team[7]), by_team[7][0])
