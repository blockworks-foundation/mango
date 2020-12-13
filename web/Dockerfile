FROM node:15-alpine as build

WORKDIR /usr/src/mango
COPY package.json yarn.lock ./
RUN yarn
COPY public ./public
COPY src ./src
RUN yarn build

# runtime image
FROM nginx:alpine

COPY --from=build /usr/src/omega/build /usr/share/nginx/html

EXPOSE 80
CMD ["nginx", "-g", "daemon off;"]
