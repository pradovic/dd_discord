# Direct Decisions Discord Bot

This is a discord bot that enables preferential polls/election using [Direct Decisions](https://directdecisions.com) v1 API.

You can view Direct Decisions API v1 docs here: [https://api.directdecisions.com/v1](https://api.directdecisions.com/v1)

It enables direct democracy style polls/elections. Results are calculated using [Schulze method](https://en.wikipedia.org/wiki/Schulze_method).

## Features
- Create votings
- Delete votings
- Vote with a ballot
- Complete voting and publish/follow results

## TODO

- OAuth & installation flow before publish
- Voting management
- Continious votings
- Improving UX

## Status

The development is still in progress. Currently application supports minimum set of features to enable preferential votings. Current implementation does not deal too much with db scalability and preformance. The db being used and evaluated is `redb` embedded database.

## License

This library is distributed under the BSD-style license found in the [LICENSE](LICENSE) file.
