# Mango web frontend

## Development setup

Run `yarn` to install dependencies, then run `yarn start` to start a development server or `yarn build` to create a production build that can be served by a static file server.

See the [Create React App documentation](https://facebook.github.io/create-react-app/docs/getting-started) for other commands and options.

## Docker setup for hosting

Run `docker build -t mango-web .` to build the container. Then run
`docker run -p 3000:80 mango-web` to start a nginx server listening on
port 3000. Open `http://localhost:3000` to access the frontend.

