  git add -A
  git commit -m "fix(publish): bootstrap checksum via force_build in publish-ex"
  git push origin main
  git push origin :refs/tags/v0.2.2
  git tag -f v0.2.2
  git push origin v0.2.2

