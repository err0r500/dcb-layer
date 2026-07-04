  git add -A
  git commit -m "fix(publish): native NIF builds + strip v from artifact version"
  git push origin main
  git push origin :refs/tags/v0.2.2
  git tag -f v0.2.2
  git push origin v0.2.2

