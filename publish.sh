  git add -A
  git commit -m "fix(publish): drop x86_64-darwin, use empty use-cross to skip cross install"
  git push origin main
  git push origin :refs/tags/v0.2.2
  git tag -f v0.2.2
  git push origin v0.2.2

