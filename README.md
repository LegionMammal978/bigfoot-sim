# bigfoot-sim

This program simulates the evolution of “Bigfoot”, a 3-state 3-symbol Turing machine. It can be established heuristically that this machine never halts, but it cannot easily be proven for certain. More information can be found in the article “[BB(3, 3) is Hard](https://www.sligocki.com/2023/10/16/bb-3-3-is-hard.html)” by Shawn Ligocki.

The basis of this program is the reduced representation $C(a,b)=A(a,4b+1,2)$, which can be expressed in the form $C(a,81k+r)\to C(a+\Delta a,256k+s)$ given an 81-entry table of $(r,\Delta a,s)$ entries. Each iteration of $C$ corresponds to 4 iterations of $A$. To speed up processing, the program recursively operates in a cycle of decomposing $b=81^{16\cdot2^p}k+R$, performing $16\cdot2^p$ iterations starting with $C(a,R)$ to obtain $S$, and recombining $b'=256^{16\cdot2^p}k+S$.

The program should be invoked as `bigfoot-sim logfile.txt`. The log file can be set to `/dev/null`. At the start of every cycle, the program will write a line to the log file with the 0-based cycle number (i.e., the reduced iteration count divided by 16), followed by the values of $a$, $b\bmod256^{16}$, and $b\bmod81^{16}$. (Note that the program always operates on the reduced value of $b$. The full $A(a,b,c)$ representation can be recovered by computing $4b+1$.) Every second, it will write a line to the standard output stream with the full iteration count (i.e., the reduced iteration count times 4), followed by the values of $a$ and $b\bmod81^{16}$.

This program and its documentation are dedicated to the public domain under [CC0 1.0 Universal](https://creativecommons.org/publicdomain/zero/1.0/).
