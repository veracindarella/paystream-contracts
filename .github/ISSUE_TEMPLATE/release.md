---
name: Release Checklist
about: Use this template to track release steps and ensure all requirements are met
title: 'Release v{VERSION}'
labels: release
assignees: ''
---

## Release Version: v{VERSION}

Replace `{VERSION}` with the actual version number (e.g., v1.2.3).

## Pre-Release Checklist

### SC-01 Verification
- [ ] SC-01 (Reentrancy Guard) verified: No reentrant paths exist in the call graph
- [ ] Reentrancy guard implementation reviewed and confirmed correct
- [ ] Cross-contract call analysis documented

### Testing
- [ ] All unit tests pass (`make test`)
- [ ] Integration tests pass (`make integration-test`)
- [ ] Linting passes (`make lint`)
- [ ] Formatting check passes (`make fmt-check`)
- [ ] Deny check passes (`make deny`)

### Audit Status
- [ ] External audit completed (if applicable)
- [ ] Audit findings addressed
- [ ] Audit report linked in release notes

### Testnet Deployment
- [ ] Contract deployed to testnet
- [ ] Testnet deployment verified
- [ ] Testnet deployment hash recorded: `_______________`
- [ ] Basic functionality tested on testnet

## Release Steps

### Documentation
- [ ] CHANGELOG.md updated with release notes
- [ ] Version bump in Cargo.toml (if applicable)
- [ ] API documentation updated (if breaking changes)
- [ ] Event schema documentation updated (if event changes)

### Git Operations
- [ ] All changes committed to main branch
- [ ] Git tag created: `git tag -a v{VERSION} -m "Release v{VERSION}"`
- [ ] Tag pushed to remote: `git push origin v{VERSION}`

### GitHub Release
- [ ] GitHub release created from tag
- [ ] Release notes populated from CHANGELOG
- [ ] Testnet deployment hash included in release notes
- [ ] Audit report linked (if applicable)

### Post-Release
- [ ] Mainnet deployment completed (if applicable)
- [ ] Mainnet deployment hash recorded: `_______________`
- [ ] Announcement posted (Twitter/Discord/etc.)
- [ ] Version updated in documentation

## Notes

Add any additional notes or context specific to this release:

<!-- 
  Example notes:
  - Breaking change: Stream status enum modified
  - New feature: Batch stream creation added
  - Bug fix: Fixed claimable calculation overflow
-->

## Sign-off

- [ ] I have completed all applicable checklist items above
- [ ] Release ready to proceed
