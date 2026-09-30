<p align="center">
  <img src="assets/bellycoin-logo.png" width="200" alt="Bellycoin">
</p>

<h1 align="center">Bellycoin</h1>

<p align="center">
  A lightweight proof-of-work cryptocurrency.
</p>

### Nakama names

Register or renew a name with `wallet register-name --name alice --years 2`.
Names contain 1–32 lowercase ASCII letters. Each year is 525,600 blocks;
the registration burn is the name-length price multiplied by `--years` (1–100).
An expired name stops resolving. Its previous owner can renew during the next
43,200 blocks; afterward anyone can register it.
Transaction history shows the name burn separately from the total outgoing amount.
