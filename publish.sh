  git add -A
  git commit -m "fix(publish): run publish-ex in dev env so ex_doc docs task is available"
  git push origin main
  git push origin :refs/tags/v0.2.2
  git tag -f v0.2.2
  git push origin v0.2.2

