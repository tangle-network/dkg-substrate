import psutil
import sys
import matplotlib.pyplot as plt
from matplotlib import animation
from collections import deque
import numpy as np

if len(sys.argv) < 2:
	print("Error: PID not specified")
	sys.exit(1)

pid = int(sys.argv[1])

try:
	process = psutil.Process(pid)
except psutil.NoSuchProcess:
	print(f"No process found with PID: {pid}")
	sys.exit(1)

fig = plt.figure()
ax = fig.add_subplot(1, 1, 1)
xs = deque(maxlen=10000)
ys = deque(maxlen=10000)


def animate(i):
	mem = process.memory_info().rss / (1024**2)

	xs.append(i)
	ys.append(mem)

	ax.clear()
	ax.plot(xs, ys)

	if len(xs) > 1:
		m, b = np.polyfit(xs, ys, 1)
		print(f"Slope: {m}")
		reg_line = [m*x + b for x in xs]
		ax.plot(xs, reg_line, color='red')


ani = animation.FuncAnimation(fig, animate, interval=1000)
plt.show()
